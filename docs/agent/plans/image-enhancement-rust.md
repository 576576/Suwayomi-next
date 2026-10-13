# 图像增强能不能搬到 Rust 侧（2026-09-30 调研）

承接 [`modelpack-compat.md`](https://github.com/576576/Suwayomi-ext-runtime/blob/main/docs/agent/plans/modelpack-compat.md)：那篇讲的是**沙盒加载面**为什么接不住
TachiyomiX 的 modelpack；这篇只回答一件事 —— **把「大模型图像增强」这个能力放到 Rust 侧
（`Suwayomi-next`）可不可行、接缝在哪、代价是什么**。

**本文只调研，不含实现。** 结论里带「建议」的都是待决策项，不是已定方案。

## 结论先行

- **能，而且不用搬 TachiyomiX 的代码。** 值得搬的是「解码 → 跑模型 → 重编码」这个**能力**，
  不是它那套 Java + arm64 `.so`。它的代码在桌面 x86-64 上一行都跑不了（见
  [`modelpack-compat.md`](https://github.com/576576/Suwayomi-ext-runtime/blob/main/docs/agent/plans/modelpack-compat.md) §四）。
- **上游早就留好了接缝，而且是协议级的。** Suwayomi-Server 的
  `ConversionUtil.imageHttpPostProcess` —— 「把页面图 POST 给一个外部 HTTP 服务，拿回处理后的图」
  —— 已经是**进程外**设计。一个 Rust worker 今天就能按这个协议接进去，不用改服务端一行。
- **Rust 侧是「契约在、行为不在」。** `SettingsDownloadConversionType` 的 6 个字段与参考实现
  **逐字段一一对应**，但 `download_conversions` / `serve_conversions` 在 Rust 侧**零消费者**
  （实测 grep 为空）。所以这不是从零设计，是**填一个已经声明好的接口**。
- **真正的成本在推理引擎，不在接缝。** ncnn 的 Rust 绑定薄且停更（`ncnnrs` 全crate 867 行、
  最后更新 2024-09；`rust-ncnn` 停在 2022）。建议**不走 ncnn FFI，走 ONNX**：
  `ort`（ONNX Runtime 绑定）活跃维护、MIT/Apache-2.0、Windows x86_64 + Linux 都支持，
  后端含 DirectML / CUDA / oneDNN / OpenVINO / QNN。
- **「资源包用 zip + 云端仓库」方向对，而且格式已经存在。** TachiyomiX 自己就是一个
  **keiyoushi 形状**的模型仓库（`/index.json` + `model-packs/` + `raw.githubusercontent.com`
  + `api.github.com/repos/`），OpenModelDB 是上游权威元数据源（含 `license` 与 `sha256`）。
  `zip` crate 也**已经在 `Suwayomi-next` 的依赖里**（CBZ 用）。
- **但有两个硬约束，都会改变方案形状：**
  1. **许可证不齐**：已核实的 4 个模型里 **2 个是 NC（禁商用）**，其中 1 个还是 SA（传染性）。
     公开仓库必须先解决这个，不能等做完再说。
  2. **TachiyomiX 的 ncnn 权重是 BGR 重排过的**（它的 ATTRIBUTION 自己写明），
     **不能直接喂给通用 runner**。反而印证了「从上游原始模型重新转换」是正确路线。

## 一、先纠正一个前提：参考实现的「图片处理」是什么

用户描述是「本项目没实现参考实现 Suwayomi-Server 的图片处理」。实测下来，
Suwayomi-Server 的图片相关代码只有四处，**没有缩放、没有缩略图生成、没有增强**：

| 文件 | 做什么 |
|---|---|
| `manga/impl/util/storage/ImageUtil.kt` | 魔数嗅探 7 种格式（JPEG/PNG/GIF/WEBP/AVIF/HEIF/JXL） |
| `manga/impl/util/storage/ImageResponse.kt` | 落盘缓存 + MIME 推断 |
| `util/ConversionUtil.kt` | **转码**（ImageIO 重编码 + `compressionLevel`）+ **HTTP 后处理** |
| `manga/impl/Page.kt` | 编排：下载/服务页面时套用转换 |
| `source/local/image/LocalCoverManager.kt` | 本地封面文件管理 |

（`ImageIO`/`TwelveMonkeys` 依赖只用于**解码**，不是增强。`handleMangaThumbnail` 只是
「把封面下载下来」，不生成缩略图 —— 缩放是 WebUI 客户端侧做的。）

所以「参考实现有、本仓没有」的准确说法是 **转码 + 后处理钩子**，不是增强。
增强在**两边都是新东西** —— 这对项目是好事：没有历史包袱要对齐。

## 二、Rust 侧现状：契约在，行为不在

### 六字段一一对应

`Suwayomi-next` 的 `crates/suwayomi-graphql/src/settings.rs`：

```rust
pub struct SettingsDownloadConversionType {
    pub call_timeout: Option<DurationScalar>,
    pub compression_level: Option<f64>,
    pub connect_timeout: Option<DurationScalar>,
    pub headers: Option<Vec<SettingsDownloadConversionHeaderType>>,
    pub mime_type: String,
    pub target: String,
}
```

参考实现 `suwayomi.tachidesk.graphql.types.SettingsDownloadConversion`：

```kotlin
val mimeType: String
val target: String
val compressionLevel: Double?
val callTimeout: Duration?
val connectTimeout: Duration?
val headers: List<SettingsDownloadConversionHeader>?
```

**六个字段、六个可空性，完全一致。** 注释也写明「Mirrors `SettingsDownloadConversionType`」。

### 但没有任何消费者

```
$ grep -rnE "download_conversions|serve_conversions" crates/*/src --include="*.rs" \
    | grep -v "mutation_b4\|settings.rs"
（空）
```

设置能经 GraphQL 存下来、能读回去，**没有任何代码读它去做事**。

### Rust 侧已有的图片面

| 位置 | 现状 |
|---|---|
| `suwayomi-rest/src/routes/image.rs`（101 行） | `/api/v1/image/{b64url}` 封面代理 + FNV-1a 落盘缓存（`{key}.img` + `{key}.mime`） |
| `suwayomi-domain/src/download.rs` | 页面下载：`resp.bytes()` → `image_ext_from_content_type()` 魔数嗅探 → 写 `{idx}.{ext}` |

**结论：Rust 侧有嗅探、有缓存、有设置契约，缺的是「变换」这一环本身。**

## 三、上游留好的接缝（本文最重要的一条）

`ConversionUtil.imageHttpPostProcess` 的行为：

```
POST <conversion.target>            # target 以 http:// 或 https:// 开头
  multipart/form-data
  field "image" = 原始图片字节（带 mimeType）
  headers: conversion.headers 逐条 set
  timeouts: conversion.callTimeout / connectTimeout
→ 响应体即「处理后的图片」，再由 ImageUtil.findImageType 嗅探 MIME
```

`target` 是**二义的**，这是整个设计的关键：

| `target` 形态 | 走哪条路 | 谁执行 |
|---|---|---|
| `image/webp` 这类 MIME | `convertToFormat()` —— ImageIO 解码后按 `compressionLevel` 重编码 | 服务进程内 |
| `http(s)://…` | `imageHttpPostProcess()` —— 发给外部服务 | **进程外** |

**这意味着：增强完全可以做成一个独立的 Rust 二进制**，服务端只把它当「HTTP 转换器」。
不需要把推理塞进 `suwayomi-server` 进程，不需要 `ort` 进主 crate 的依赖树，
也不需要解决「主进程要不要背 200 MB 运行时」这个问题。

`serveConversions`（`Page.kt:155`）走的是同一个 `convertImageResponse`，所以
**下载期增强**和**服务期增强**共用一条实现，只是挂载点不同。

## 四、要搬的是哪三层，各自难度

| 层 | 内容 | 难度 | 说明 |
|---|---|---|---|
| ① 模型数据 | ncnn `.bin` + `.param` | **低** | 格式标准、可移植。但见下方 BGR 陷阱 |
| ② 推理引擎 | ncnn + Vulkan / QNN | **高 —— 唯一的墙** | arm64 `.so` 在桌面用不了，必须换运行时 |
| ③ 编解码 | 读 JPEG/PNG/WebP/AVIF → RGBA → 写回 | **中** | Rust 侧目前**一个图片 codec 依赖都没有** |

### ① 的陷阱：TachiyomiX 的权重不是原味 ncnn

两个带 ATTRIBUTION 的包自己写明了：

> The RGB normalization, input, and PixelShuffle output channel groups are **reordered to BGR**
> for this application's Vulkan image pipeline.

也就是说这些 `.bin`/`.param` 是为**它的** Vulkan 管线改过通道序的。**通用 runner 直接吃会得到
颜色错乱的结果。** 这反过来支持「不必从 APK 转」的判断 —— 从上游原始模型重新转换，
既避开 BGR 陷阱，也避开「他的转换是哪个版本的」这个问题。

### ③ 是容易被低估的一层

Rust 的 `image` crate 对 JPEG/PNG 没问题，但 **WebP / AVIF / JXL 的支持是分层且可选的**，
而漫画页面这三种都很常见。参考实现是靠 TwelveMonkeys + `webp-imageio` 撑起格式广度的。
这一层要单独评估，别默认「加个 `image` 就完事」。

## 五、推理引擎选型（Rust 侧）

| 方案 | 状态（实测/查证） | 评价 |
|---|---|---|
| **`ort`（ONNX Runtime）** | v2.0.0-rc.13，**2026-07-28** 发布，MIT OR Apache-2.0，~2,000 万下载，13,117 行 Rust，79 文件 | **首选。** 活跃维护；Windows x86_64 + Linux 都支持；后端含 DirectML / CUDA / oneDNN / OpenVINO / QNN。注意：**无 stable 版**，1.x 系列全部 yank，只有 `2.0.0-rc.*` 可用 |
| `ncnnrs` | v0.1.7，**2024-09-12**，MIT，**全 crate 867 行** | 薄绑定，不是「ncnn 的 Rust 实现」（描述有夸大）；且需预编译 ncnn 静态库。**停更 2 年** |
| `tpoisonooo/rust-ncnn` | 文档停在 2022 | 同上，更旧 |
| `tract`（纯 Rust） | — | 无外部运行时，但超分模型的自定义 op（PixelShuffle 等）能否覆盖要逐个验 |

**建议路线：从上游原始 PyTorch 模型转 ONNX，用 `ort` 跑，不碰 ncnn FFI。**

理由：
1. 上游模型本来就是 PyTorch（OpenModelDB 的 `resources[].platform == "pytorch"`），
   到 ONNX 是成熟路径；到 ncnn 反而要经过 chaiNNer 之类的转换器。
2. `ort` 的维护状态和平台覆盖都远好于 ncnn 绑定。
3. `ort` 自带 `qnn` / `directml` / `coreml` / `nnapi` 后端 —— 如果将来要回到 Android，
   同一套模型可以复用（TachiyomiX 走 QNN 的路子 `ort` 也支持）。

**反过来说**：如果坚持复用 TachiyomiX 现成的 ncnn 权重，就同时要接受
（a）BGR 重排、（b）薄且停更的绑定、（c）自己交叉编译 ncnn 静态库。三条都不划算。

## 六、资源包用 zip + 云端仓库

### zip 的能力已经在了

`Suwayomi-next` 的 workspace 依赖里已经有：

```toml
# Local-source archive chapters (ZIP/CBZ read support; deflate only).
zip = { version = "2", default-features = false, features = ["deflate"] }
```

而且 `reqwest` 已经开了 `multipart` 特性 —— 正是 §三 那条接缝需要的能力。
**容器和传输两件事都不用新增依赖。**

### TachiyomiX 已经是一个 keiyoushi 形状的模型仓库

从主 APK 的 dex 里实测到的字符串：

```
/index.json
model-packs/
https://raw.githubusercontent.com/
https://raw.githubusercontent.com/(.+?)/(.+?)/.+     ← 用来解析仓库 URL 的正则
https://api.github.com/repos/
apkUrl / archiveSha256 / archiveSize / browser_download_url / updatedIndexUrl
num_repos / reposCount / action_open_repo
```

以及一条现成的「远端模型包」先例 —— 深度模型是**按需下载的 zip**：

```
https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/
  depth_anything_v3/releases/v0.61.0/depth_anything_v3-qnn_dlc-float.zip
```

对应的 ATTRIBUTION 也写明「The model is downloaded on demand and is not embedded in this application.」

**所以「云端搭一个模型仓库、用 zip 分发」不需要发明格式** —— 照抄 TachiyomiX 的
`/index.json` + `model-packs/<id>.zip` 即可，而它本身就是照抄 keiyoushi 的。

### keiyoushi 的真实 schema（实测）

`index.min.json` 是**裸数组**（无外层包装），单条目：

```json
{
  "name": "Outdated App",
  "pkg": "eu.kanade.tachiyomi.extension.all.keiyoushi",
  "apk": "tachiyomi-all.keiyoushi-v1.4.1.apk",
  "lang": "all",
  "code": 1,
  "version": "1.4.1",
  "nsfw": 0,
  "sources": [{ "name": "...", "lang": "all", "id": "1", "baseUrl": "https://..." }]
}
```

**注意：没有 sha256 字段。** （完整 `index.json` 是 `extensionList.extensions[]` 包装，
资源地址在 `resources.apkUrl` —— 见 [`sandbox.md`](https://github.com/576576/Suwayomi-ext-runtime/blob/main/docs/agent/sandbox.md) 与台架技能里的记录。）

模型仓库应当**加上** `sha256` —— keiyoushi 不带校验，但模型是几 MB～几十 MB 的二进制，
值得带。

### 上游权威元数据源：OpenModelDB

两个 ATTRIBUTION 都指向 `openmodeldb.info`，实测其数据源是
`OpenModelDB/open-model-database` 仓库的 `data/models/<id>.json`，单模型条目：

```json
{
  "name": "sudo UltraCompact",
  "author": "sudo",
  "license": "CC-BY-NC-SA-4.0",
  "tags": ["anime", "cartoon", "restoration"],
  "architecture": "compact",
  "size": ["64nf", "8nc"],
  "scale": 2,
  "inputChannels": 3,
  "outputChannels": 3,
  "resources": [{
    "platform": "pytorch", "type": "pth",
    "size": 1226766,
    "sha256": "e53987f0312dee424b4dbd9dce7b2eacbe03fdf1380e44a11f8a4d2ca88c99e3",
    "urls": ["https://objectstorage.us-phoenix-1.oraclecloud.com/..."]
  }],
  "pretrainedModelG": "2x-realesrganv2-animevideo-xsx2"
}
```

**这套 schema 就是模型仓库该有的样子**：`license` + `author` + `architecture` + `scale` +
`resources[].sha256` + `resources[].urls`。600+ 模型、社区维护、有 `docs/licenses.md` 定义
许可证分类。

### 溯源实测：能对上，也能对不上

把包内 ATTRIBUTION 的 sha256 与 OpenModelDB 的 `resources[].sha256` 直接比：

| 包 | 包内 ATTRIBUTION | OpenModelDB | 结论 |
|---|---|---|---|
| `sudo-ultracompact` | `e53987f0…99e3` | `e53987f0…99e3` | ✅ **逐字相同** |
| `span-nomosuni` | `a3d35e01…012a` | `bc5fa00b…6bb6` | ❌ **对不上** |

span 那条还有一个佐证：包里的 `.bin` 是 **1,642,900 B**，而 OpenModelDB 的 `.pth` 是
**8,951,347 B**（≈5.4×）；相比之下 sudo 的 `.bin` 1,218,904 B 对 `.pth` 1,226,766 B（≈0.99×），
和「preserves the original float32 weights」的说法吻合。span 的比例不像同一份权重。

**不下结论**（可能是 OpenModelDB 条目更新过、可能是换了来源、也可能 `.bin` 是另一种精度），
但这条实测说明两件事：

1. **跨源核对是有效的** —— 一半对得上、一半对不上，说明这个校验真的在筛东西。
2. **模型仓库必须带 sha256 + 一个校验步骤**，否则「这个包是哪来的」永远说不清。
   从上游原始模型自己转换 + 自己记 sha256，比继承别人的转换结果更可控。

## 七、许可证：能不能公开分发

已核实的部分：

| 模型 | 许可证 | 商用 | 来源 |
|---|---|---|---|
| `2x-sudo-UltraCompact` | **CC-BY-NC-SA-4.0** | ❌ 禁商用 | 包内 ATTRIBUTION + OpenModelDB |
| `2x-NomosUni-span-multijpg-ldl` | CC-BY-4.0 | ✅ | 包内 ATTRIBUTION + OpenModelDB |
| `2x-AnimeJaNai-v2-UltraCompact` | **CC-BY-NC-SA-4.0** | ❌ 禁商用 | 主 APK 内 ATTRIBUTION |
| Depth Anything V3 (DA3-Small) | Apache-2.0 | ✅ | 主 APK 内 ATTRIBUTION |
| Real-CUGAN (bilibili/ailab) | MIT | ✅ | 上游 LICENSE |
| Real-ESRGAN (xinntao) | BSD-3-Clause | ✅ | 上游 LICENSE |
| Anime4K (bloc97) | MIT（© 2019 bloc97） | ✅ | 上游 LICENSE |

> 注：**Anime4K 严格说不是「模型」** —— 上游是一组 GLSL 着色器算法，没有权重文件。
> TachiyomiX 那个 `anime4k` 包是把它**用 C 重新实现**在 `libmodelpack.so` 里的
> （`anime4k_process` / `anime4k_power_function`，`modelpack_describe` 自称
> "Anime4K edge-directed 2x"），所以那个包 41 KB 就装下了全部东西。
> 这也意味着：Anime4K 这条线**没有「模型仓库」可言**，要么重写算法，要么不收录。

OpenModelDB 的许可证分类（`docs/licenses.md`）：`CC0-1.0` / `CC-BY-4.0` / `CC-BY-NC-4.0` /
`CC-BY-SA-4.0` / `CC-BY-NC-SA-4.0`。其中后两个带 **ShareAlike（传染）**，NC 两个**禁商用**。

**一条关键的澄清**（OpenModelDB 原文）：

> the license of a model does **not** extend to the upscaled images produced by the model.
> a model under a non-commercial license can be used to upscale images that are then used commercially.

所以 NC 限制的是**分发模型本身**，不是增强后的输出图。对「仓库能不能公开托管这些权重」
是硬约束；对「用户拿增强结果干什么」不是。

**这条会影响仓库设计**：索引里必须有 `license` 字段，客户端要么按许可证过滤，
要么在界面上明确标注。不能像 keiyoushi 那样只标 `nsfw: 0/1` —— 维度不一样。

## 八、最小可行路径（若将来做）

按「先不引入风险 → 再验证能力 → 最后才谈产品化」分层。

**L0 —— 只做进程外服务，不碰主仓**
一个独立 Rust 二进制，实现 §三 那条 multipart 协议（`POST /` + field `image` → 返回图片）。
服务端只需把 `downloadConversions` / `serveConversions` 的 `target` 配成它的 URL。
**这一层完全不需要改 `Suwayomi-next` 的代码**，也不需要决定许可证问题（本地自用）。
代价：需要先解决 §四 的 ② 和 ③。

**L1 —— 打通设置到行为**
让 `download_conversions` / `serve_conversions` 真正被消费（转码路径 + HTTP 后处理路径）。
这是「补齐与参考实现的差异」，本身独立于增强，**即使不做增强也值得做**。

**L2 —— 模型仓库**
照 §六 的形状搭：`index.json`（带 `license` / `sha256` / `scale` / `architecture`）+
`model-packs/<id>.zip`。模型**从上游原始权重自己转换**，不继承 TachiyomiX 的 BGR 版。

**明确不做**：
- 不把 TachiyomiX 的 arm64 `.so` 或它的 ncnn 权重直接搬过来（§四 ① + [`modelpack-compat.md`](https://github.com/576576/Suwayomi-ext-runtime/blob/main/docs/agent/plans/modelpack-compat.md) §四）。
- 不把 `ort` 塞进 `suwayomi-server` 主进程（§三 的进程外接缝就是为了避免这个）。
- 不在许可证问题解决前公开托管 NC 模型。

## 九、待办 / 决策点

- [ ] **先决策路线**：走 `ort` + ONNX（建议）还是坚持 ncnn FFI。这个决定决定后面全部工作量。
- [ ] **确认 `ort` 的 rc 状态可接受**：无 stable 版，1.x 全部 yank。要不要锁 `2.0.0-rc.13`。
- [ ] **编解码层单独评估**（§四 ③）：WebP/AVIF/JXL 在 Rust 侧怎么覆盖。
- [ ] **许可证策略**：公开仓库是否收录 NC 模型？如果收录，客户端如何呈现？
      这一条不解决，L2 不能启动。
- [ ] **确认 `serveConversions` 在参考实现里的完整语义**（本次只确认了它在 `Page.kt:155` 被读取，
      没有逐行读它的分支）。
- [ ] L1（补齐转换行为）是否独立排期 —— 它与增强解耦，是「对齐参考实现」的欠账。

## 十、可复现的侦察命令

```bash
# 路径约定：<ref>/   = 参考实现 Suwayomi-Server 的检出根
#           <next>/  = Rust 侧重写 Suwayomi-next 的检出根

# 参考实现的图片面
grep -rniE "resize|downscale|thumbnail|ImageIO|BufferedImage" \
     <ref>/server/src/main/kotlin --include="*.kt"
sed -n '255,345p' <ref>/server/src/main/kotlin/suwayomi/tachidesk/manga/impl/Page.kt
cat <ref>/server/src/main/kotlin/suwayomi/tachidesk/util/ConversionUtil.kt

# Rust 侧：契约在、行为不在
grep -rnE "download_conversions|serve_conversions" \
     <next>/crates/*/src --include="*.rs"
sed -n '130,165p' <next>/crates/suwayomi-graphql/src/settings.rs
grep -rniE "^image|fast_image|libwebp|ravif|ncnn|ort|onnx" \
     <next>/crates/*/Cargo.toml

# 上游模型元数据（权威）
curl -s https://raw.githubusercontent.com/OpenModelDB/open-model-database/main/data/models/2x-sudo-UltraCompact.json
curl -s https://raw.githubusercontent.com/OpenModelDB/open-model-database/main/docs/licenses.md

# 包内 ATTRIBUTION（provenance 原始记录）
#   <pack>.apk → assets/<assetPath>/ATTRIBUTION.txt
#   主 APK → assets/{animejanai-ncnn-vulkan/*,spatial-depth}/ATTRIBUTION.txt

# TachiyomiX 的仓库形状（dex 字符串）
#   /index.json、model-packs/、raw.githubusercontent.com、api.github.com/repos/
```

## 维护约定

同 [`README.md`](README.md)：已定的选型连日期一起留着；落地之后不删文档、把结果补成一节；
与代码现状对不上的口径当场改掉。
