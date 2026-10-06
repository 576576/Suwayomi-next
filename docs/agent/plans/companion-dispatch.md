# 伴生仓联动：手动 dispatch 先拉起 ext-runtime / tray 的对应构建

主仓的发布要捆两份**别的仓库**发布的资产（ext-runtime 的沙盒 jar + 裁剪 JRE + 共享源码、tray 的六个桌面壳），
而那两个仓的**自动 alpha 只出最小发货集** —— 于是"全平台 + `windows_toolchain=all`"这类组合必然缺件，
缺件的表现不是一句报错，而是各自 job 里的 404。本文给的是"让主仓的一次 dispatch 把两仓的对应构建也带起来"
的方案与分阶段安排。

## 1. 问题

- 两仓推 main 的自动 alpha 刻意压成本：
  | 仓 | 自动 alpha 出什么 | 出不了什么 |
  |---|---|---|
  | `576576/Suwayomi-ext-runtime` | 两个 jar + `linux-x64` / `windows-x64` 两份裁剪 JRE | 其余四份 JRE（`linux-aarch64` / `mac-x64` / `mac-aarch64` / `windows-aarch64`） |
  | `576576/Suwayomi-tray` | `windows-x64` + `linux-x64`，工具链恒 `msvc` | 其余四个 target、以及所有 `-gnullvm` 资产 |
- 主仓 prep 按通道挑制品：release → 最新**正式**（两个仓的 stable 都齐），alpha/beta → 最新**预发布**。所以
  「alpha + 全平台 + `pack_jre`」与「alpha + `windows_toolchain=all` + `pack_msi`」这两条组合里，
  缺的资产是在**取件那一刻**才炸：
  - 桌面矩阵 5 项（mac×2 / linux-arm64 / windows-arm64×2）下载 JRE 404；
  - `windows-arm64` / `windows-x64-gnullvm` / `windows-arm64-gnullvm` 取不到桌面壳 → `HAS_TRAY=no` → WiX ICE64；
  - 桌面矩阵一挂，`publish` 被 skip → **整轮不出 Release**（2026-10-03 run `37123510190`：9 绿 / 7 红 / 2 跳过）。
- 现状是**人肉前置**：先去 ext-runtime 手动 dispatch（`channel=alpha`、`build_jre=true`、`publish_packages=false`），
  再去 tray 手动 dispatch（`windows_toolchain=all` + 六个平台全勾），等两边出完，再回主仓跑全量。
  可行，但"参数对不对 / 出没出完"全靠人记。
- **注**：2026-10-03 那轮里 android 取件与 OCI 的两处 404 是**另一个问题**（把裸版本号当 release tag 用，
  与资产覆盖无关），已于 2026-10-06 修好并在 CI 上实证（定向 run `37407805350`：android×2 + OCI×2 + manifest
  全 success）。本文要处理的是剩下的那一类 —— **伴生仓根本没产出所需资产**。

## 2. 目标与非目标

- 目标：主仓**一次** `workflow_dispatch` 就能端到端跑通（含全平台 alpha），不必先到别处点两下；
  且"资产没齐"要在**取件之前**变成一句能照着做的错误，而不是 5~7 个 job 各自 404。
- 非目标：不改三个仓各自的版本号规则与产物命名；不做与本次选择无关的通用编排（比如"任意仓任意选项"）。

## 3. 前置核实：两仓的 dispatch 选项现状

| 仓 | `workflow_dispatch` 输入 | 出全量所需的取值 |
|---|---|---|
| Suwayomi-ext-runtime | `channel` / `build_jre` / `publish_packages` / `version_count` / `release_notes` | `channel=alpha` + `build_jre=true`（六份 JRE）+ `publish_packages=false`（不为一次 alpha 堆一个不可变的 Maven 版本） |
| Suwayomi-tray | `channel` / `windows_toolchain`（msvc\|gnullvm\|all）/ 六个平台开关 / `version_count` / `release_notes` | `channel=alpha` + `windows_toolchain=<主仓那支>` + 六个平台全勾（手动默认值就是全勾） |

- **tray 的 `windows_toolchain` 三选已经在 main 上**（`2b3f4d5`，2026-09-30），`all` = 每个被勾选的 Windows
  target 各出两份、msvc 在前，`gnullvm` 那份另发一份同名换扩展名的 `WebView2Loader.dll`。所以「tray 仓缺少
  gnullvm 选项」这个前提不成立 —— 缺的是**自动化**（主仓不去问、也不检查，就得人手去点）。
  唯一没有它的是 tray 的**自动** alpha（恒 `msvc`、只出两个 target），那是刻意压成本的。
- ⚠️ 但托盘仓的 **gnullvm 分发路径一次都没在 CI 跑过**（至今没有一个 `-gnullvm` 资产）。本地能用 llvm-mingw
  编出可运行的 gnullvm 托盘，CI 侧却未经实测 —— 所以自动化第一次跑起来时，要盯的就是这一步。
- 排队：ext-runtime 的 `release.yml` 有 `concurrency: release-<ref>`（`cancel-in-progress: false`），
  两次派发会排队；**tray 没有** `concurrency`，同时派发会并行跑同一批 target（浪费但无害）。
- 消费侧已就绪：`ext_runtime_tag` 这个值 2026-10-06 已经打通（见「已落地」），`resolve-ext-runtime.sh` 的位置参数
  按 **tag** 全等匹配；`resolve-tray.sh` 的匹配口径相同，但**还留着"参数不能带 `v` 前缀"的报错分支**、
  HTML 兜底层也只会拼 `v<参数>` —— 按 tag 精确取件之前要把它对齐（见阶段 2）。

## 4. 方案 A（推荐）：预检先行 + 编排

拆成两块，**A1 不需要任何密钥**，A2 需要。

### A1 取件前覆盖检查（prep 内，无 token）

prep 拿到三个 `base` 并算完矩阵之后，核对"这次要取的资产是不是都在"：

- **裁剪 JRE**：`pack_jre=true` 时，按矩阵里出现过的 `(jre_os, jre_arch)` 逐条检查
  `<jre_base>/ext-runtime-jre-<V>-<os>-<arch>.tar.gz`。
- **桌面壳**：按矩阵里出现过的 `target` 检查 `<tray_base>/suwayomi-tray-<V>-<target>[.exe]`（gnullvm 再要 `.dll`）。
- 检查方式用**一次 `releases/tags/<tag>` 拉资产清单比名字**，不要每条 `HEAD` 一次：同一 release 的资产在一次响应里，
  API 调用数从 6~8 次降到 1~2 次，也不会被 CDN 的 302/限流干扰。
- 缺件就**在 prep 里失败**，消息里直接给出"去哪个仓、用哪些参数 dispatch"，并把还缺哪几个资产列全。

价值：把"5~7 个 job 各自 404 + `publish` 被跳过"收敛成"1 个 prep 红 + 一句可执行的指引"。
它**独立于 A2 有用** —— 就算不做自动派发，人肉前置时也不会再猜"到底齐没齐"。

### A2 `refresh_companions`：派发 + 等待 + 按 tag 取件

新增 dispatch 输入 `refresh_companions`（boolean，**默认关**，已拍板）。开着时新增一个 `companions` job（在 `lint`/`prep`
之前跑完，或与 `prep` 串行）：

1. 按主仓本次的选择推导两仓的 dispatch 参数（见 §3 的表）：工具链 1:1 传、平台开关按主仓选中的桌面目标映射。
2. 用 PAT 调 `POST /repos/<owner>/<repo>/actions/workflows/release.yml/dispatches`（`ref: main`，带 `inputs`）。
3. 等它跑完：`GET /actions/runs?event=workflow_dispatch` 过滤出自己那条（`created_at >= 派发时刻`，同 ref），
   轮询 `status`/`conclusion`；带超时（60 min 量级）与"失败即中止"。
4. 出 tag：**按 run id 反查** —— alpha 的 tag 形如 `<版本名>-alpha.<run_id>`，版本名各仓自己算，
   所以从 releases 列表里挑 tag 以 `-alpha.<该 run id>` 结尾的那条。**不要"取最新"**：派发到查询之间
   可能有另一条自动 alpha 插进来。
5. 两个 tag 交给取件：`ext_runtime_tag`（已有）+ 新增 `tray_tag`，解析步骤改成按 tag 精确取；
   `resolve-tray.sh` 补上和 `resolve-ext-runtime.sh` 同款的两处对齐（去掉 `v` 前缀报错分支、HTML 兜底层
   两种 tag 形态都试）。

**密钥**：`github.token` 只能操作本仓，跨仓 dispatch 必须另配 `COMPANION_DISPATCH_TOKEN`
（fine-grained PAT，**只授权这两个仓**的 `Actions: write` + `Contents: read`/`Metadata: read`；classic PAT 要 `repo` + `workflow`），
存成主仓的 repository secret。这是 A2 唯一新增的基础设施。

**为什么是 `workflow_dispatch` 而不是 `repository_dispatch` 或其他触发**：两者都要 token，而
`workflow_dispatch` 能**带输入** ——"对应选项"（工具链、平台、出不出六份 JRE）正是本方案的核心。

**失败语义**：compaion 失败/超时 = **整轮失败**，不做"退回旧 release"的降级 —— 那正是这次踩的坑
（静默用到一份不含所需资产的旧构建）。要"用现成的、不重新构建"，就别勾 `refresh_companions`，
由 A1 判它够不够。

**通道**：只在 `alpha` / `beta` 下有意义（派发时同通道）。`release` 通道下主仓取的是两仓的**正式**版，
那是"该不该发 v3.y.z"的人为决定，`refresh_companions=true` 时直接报错说清楚。

## 5. 方案 B（备选，零密钥）：让伴生仓的自动 alpha 出全量

把两仓推 main 的自动 alpha 从"最小集"改成"全量"：tray 出六个 target（按需带 gnullvm）、ext-runtime 出六份 JRE。

- 优点：零密钥、零编排，主仓现有逻辑什么都不用改就能跑通全平台 alpha。
- 缺点：每次伴生仓提交都跑满（tray 含 macOS 10× 计费的 runner），与"自动构建刻意压成本"的既定取舍相反。
- 建议：只有在"不接受再加一个 secret"时才选它。

## 6. Windows 安装包那条硬伤（可与 A/B 独立决策）

托盘资产缺 → `HAS_TRAY=no` → `Suwayomi.wxs` 里含 `AppMenuFolder` 的 `RemoveFolder` 组件被
`<?if $(HasTray)="yes"?>` 排除 → `wix msi validate` 报 **ICE64**。三条路：

1. 把 `ICE64` 也加进 `-sice` —— 等于接受一个"卸载后留残目录"的包；
2. 让 `pack_msi` 与工具链解耦（gnullvm 不出 msi）—— 用户少一个形态，且没解决根因；
3. **把托盘资产补齐**（A 或 B 干的就是这件事）—— 补齐之后 ICE64 不再触发。

建议 3；1/2 只在"就是不想为 gnullvm 出安装包"时才考虑。

## 7. 分阶段

| 阶段 | 内容 | 依赖 | 状态 |
|---|---|---|---|
| 0 | **A1** 取件前覆盖检查（prep 内，无 token）→ `scripts/check-companion-assets.py` | — | 已完成（2026-10-06） |
| 1 | tray 仓补 `concurrency`（ext-runtime 本来就有）；两仓 dispatch 参数表写进 `docs/agent/release.md` | — | 已完成（2026-10-06） |
| 2 | **A2**：`refresh_companions` 输入 + prep 里的 `companions` 步骤 + `COMPANION_DISPATCH_TOKEN` + `ext_runtime_tag` / `tray_tag` override + `resolve-tray.sh` 对齐 tag 口径 → `scripts/refresh-companions.py` | 阶段 0/1 | 代码已完成（2026-10-06）；**首次真跑待建 secret** |
| 3 | 文档收口：`docs/agent/release.md` 补「伴生仓前置」一节；`GOTCHAS.md` 里那三条硬伤改成"已由谁解决/仍需人肉" | 阶段 2 | 已完成（2026-10-06） |
| — | **不**单独预验 tray 的 gnullvm（`windows_toolchain=gnullvm`）—— 等阶段 2 第一次真跑时一起验 | — | 已决定跳过 |

全部待定项已拍板，见 §8。仅剩的一件事是**在主仓建 secret `COMPANION_DISPATCH_TOKEN`** —— 那是
只有人能做的授权动作；不建它时 workflow 本身照样能跑（`refresh_companions` 默认关），勾了则脚本明确报错。

## 8. 已拍板（2026-10-06）

1. **接受**新增受限 PAT `COMPANION_DISPATCH_TOKEN`（fine-grained，只授权 ext-runtime / tray 两仓的
   `Actions: write` + `Contents: read` / `Metadata: read`），存成主仓 repository secret。A2 的硬前提已满足。
2. `refresh_companions` **默认关**。自动 alpha 通道一律不生效（它只出 `windows-x64` + `linux-x64` + `msvc`，
   现有资产已够）；要全量 alpha 时由人手勾。
3. A1 覆盖检查发现缺件 → **直接失败**。消息里列全缺口，并给出"去哪个仓、用哪些参数 dispatch"。
4. **不**单独预验 tray 的 gnullvm 分支，等 A2 第一次真跑时一起验。
   代价：首次跑 A2 会同时面对"编排新代码"与"gnullvm 首次在 CI 实跑"两个变量 —— 出问题先看是不是后者。

## 已落地（与本文相关的部分）

- **2026-10-06**：ext-runtime 的取件口径拆成"资产名用裸版本号、下载路径用 release tag"，并打通
  `ext_runtime_tag`（prep 输出 → `build.yml` → android 取共享源码 / OCI 的 `EXT_RUNTIME_TAG` build-arg）。
  `resolve-ext-runtime.sh` 的位置参数改为按 **tag** 全等匹配（带不带前导 `v` 都认），
  `fetch-ext-runtime-src.sh` 按 tag 校验。**这不是本文的方案，而是它的前提**：按 tag 精确定位是 A2
  第 4/5 步"派发完按 run id 反查 tag"的必要条件。CI 实证：定向 run `37407805350` 的 android×2
  （第 10 步「取 ext-runtime 共享源码」）、OCI amd64/arm64（构建 / 冒烟 / 推送）与多架构 manifest 全 success。
