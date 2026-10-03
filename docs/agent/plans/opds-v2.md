# OPDS 2.0（`/api/opds/v2`）方案与计划

给**维护者与 AI** 看：为什么要做、与 1.2 的硬边界、逐字段映射、分阶段落地、怎么验。
现状那一节（§1）是动手前必读的部分。五项选型已于 **2026-10-01 定稿**（§10）。

## 0. 结论先行

- 新端点 **`/api/opds/v2`**，与 `/api/opds/v1.2` **并列挂载**（`crates/suwayomi-server/src/lib.rs` 的 `build_router`）。两份 README 原先写着
  `OPDS: /api/opds/v1.2（In-progress: /api/opds/v2）`，阶段 6 已改成两个端点并列。
- 代码落在**同一个 crate** `suwayomi-opds` 内：新增 `src/v2/`（模型 + 序列化 + 路由 + feed 组装），
  **复用 `src/repository.rs` 这层数据访问**（它返回的是协议无关的数据载体，与 Atom 无关），只换序列化层。
- **路由形状与 1.2 一一对应**：把 URL 里的 `/api/opds/v1.2` 换成 `/api/opds/v2` 即可，19 条里去掉 2 条、
  新增 1 条（章节的 Divina 清单），共 **18 条**（§4）。
- **章节的表达走标准路线**：每个章节的 acquisition 指向一份 **Readium Divina 清单**
  （`application/divina+json`），**不用** 1.2 的 PSE 扩展位（`http://vaemendis.net/opds-pse/stream`）。
  这是 2026-10-01 定下的（§10.2），代价是多一条路由、且清单要能枚举页（§5.4）。
- **1.2 一行不动**：`router.rs` / `feeds.rs` / `model.rs` / `xml.rs` / `constants.rs` 全部不改；
  `repository.rs` 只允许**新增**查询或字段，不得改既有方法与字段语义（§3）。
- v2 相对 1.2 不是"多一批功能"，而是**同一份数据的第二种编码**，加上规范强制的一批结构约束（§6）。
  这些约束会咬人：最典型的是「每个 publication 必须至少有一条 acquisition 链接」，1.2 里用 `subsection`
  指章节列表的写法在 2.0 下**不合规**。
- **客户端现实要让决策建立在事实上**：OPDS 2.0 目前只有 Thorium Reader 被广泛验证；KOReader、Panels、
  Chunky、Calibre、Moon+ 走的都是 1.2。所以 v2 的第一价值是**规范合规 + JSON 好接第三方工具**，
  短期内不可能替代 1.2 —— 两条并存正是本次"不影响 1.2"的形态（§2）。

## 1. 现状：1.2 端点是什么样（2026-10-01）

| 文件 | 内容 |
|---|---|
| `crates/suwayomi-opds/src/router.rs` | 19 条路由 + query 解析 + 404 兜底；`BASE_URL = "/api/opds/v1.2"` 是**路径前缀而不是绝对 URL**，所以全部 href 都是相对的 |
| `crates/suwayomi-opds/src/feeds.rs` | 各 feed 组装（`FeedBuilder`）+ Atom entry 构造 |
| `crates/suwayomi-opds/src/model.rs` | Atom/OPDS 1.2 的 Feed/Entry/Link 模型 |
| `crates/suwayomi-opds/src/xml.rs` | 手写 XML 树 + 转义（无外部 XML 依赖） |
| `crates/suwayomi-opds/src/constants.rs` | 命名空间 / rel / 媒体类型 / `ITEMS_PER_PAGE = 50` |
| `crates/suwayomi-opds/src/repository.rs` | SQL：漫画、章节、导航项、漫画详情、章节元数据 |
| `crates/suwayomi-opds/tests/opds_feeds.rs` | 内存 SQLite 播种 + 断言渲染结果（v2 的测试照这个模子写） |

挂载点：`crates/suwayomi-server/src/lib.rs:160` `.nest("/api/opds/v1.2", suwayomi_opds::router::opds_router())`。

**认证不需要为 v2 做任何事**（`crates/suwayomi-api/src/auth.rs`）：

- `is_data_path` 判 `path.starts_with("/api/")` → `/api/opds/v2/**` 自动进「匿名 401」；
- `token_query_allowed` 判 `path.starts_with("/api/opds/")` → `?token=` 在 v2 上同样放行。

两条都靠**前缀**而不是精确路径，所以 v2 天然落在同一条策略里 —— 这是选 `/api/opds/v2` 而不是别的路径的额外好处。

**设置项现状**（决定 §7）：`ServerConfig` 里只有一个 `opds` 前缀的字段 `opds_cbz_mimetype`
（`crates/suwayomi-core/src/config.rs:195`），而它**只在 REST 的下载响应里生效**
（`crates/suwayomi-rest/src/routes/chapter.rs:75` 拿它写 `Content-Type`）；
OPDS 1.2 侧对 CBZ 链接写死 `TYPE_CBZ = "application/vnd.comicbook+zip"`（`constants.rs:40`）。
设置页上那 9 个 `opds*` 在 **OPDS 侧一个都不读**：`ITEMS_PER_PAGE` 是编译期常量 50，
`updateProgress=true` / `markAsRead=true` 是写死的字符串，章节排序恒 `number_asc`，
`opdsShowOnly*` / `opdsSkipChapterMetadataFeed` 全未生效。

章节的链接在 1.2 里**按 feed 分叉**（`feeds.rs:323` 的 `chapter_list_entry(..., skip_metadata)`）：

| feed | `skip_metadata` | 章节 entry 上的链接 |
|---|---|---|
| `/series/{id}/chapters` | `false` | 只有一条 `subsection` 指向 metadata 子 feed —— **没有**取页链接、**没有** CBZ |
| `/history`、`/library-updates` | `true` | `page_count > 0` 才给 PSE 取页模板；`downloaded` 才给 CBZ；另加一条首页图 |

也就是说 1.2 的章节「能不能直接读」取决于它在哪个 feed 里 —— 2.0 内联元数据后不再有这个分叉。

**实测补充（2026-10-01 抓基线时发现）**：

- 路由必须写**无尾斜杠**的形式。`/api/opds/v1.2` 与 `/api/opds/v1.2/library/series` 命中 OPDS
  （`200 application/xml;profile=opds-catalog`）；各加一个尾斜杠后不匹配 nest 路由，落到 WebUI 的
  SPA fallback，返回 `200 text/html` 的 index.html（状态码仍是 200，所以光看状态码发现不了）。
- 连带后果：1.2 每个 feed 的 `rel="self"` 都写成**带尾斜杠**的形式，**实际取到的是 WebUI**；
  分页的 `next` / `prev` / `first` / `last` 走同一个函数，同理。成因在
  `FeedBuilder::url_with`（`feeds.rs:94`）的收尾 `format!("{base}/?{q}")` —— 无条件在 `?` 前插一个
  `/`，而 `lang=` 是必加参数、`q` 永不为空，所以没有一条 self 是干净的。
- 1.2 保持现状不动 —— 改它就是改 1.2 的输出，与本方案的硬约束冲突。**v2 拼 href 时不要沿用这个
  写法**：2.0 的 `rel=self` 是 schema 强制的，但 schema 只管 href 是不是合法 URI，不管它取不取得到
  feed —— 指向一个返回 HTML 的 URL 等于没写。

文档面（改动时同步，2026-10-03 阶段 6 已落）：

- `docs/agent/rest-api.md` §6 的 18 行路由表（并写明它是本仓新增、参考实现无对应端点）；
- `docs/zh/user-guide.md` 「OPDS / KOReader」一节；
- 根 `README.md` 与 `docs/en/README.md` 各一行的 `In-progress` 已去掉，改成两个端点并列。

## 2. 规范与客户端现实（2026-10-01）

- 规范：<https://specs.opds.io/opds-2.0> —— 自称 **living standard**，正文核心自 2017 年起稳定。
  引用时用这个地址：旧的 `drafts.opds.io/opds-2.0` 已经 404，1.2 与 2.0 现在同在 `specs.opds.io` 下。
- 媒体类型：feed 是 `application/opds+json`；单个 publication 另有 `application/opds-publication+json`，
  本项目**不用**后者（章节/漫画一律内联进 feed 的 `publications`）。
- **官方 JSON Schema 才是硬约束的来源**：正文有时比 schema 松（正文说 acquisition 关系有 6 个，
  而 schema 的枚举是另一套写法，见 §6.2）。已连依赖一起抓到
  `.workbuddy/verify/schemas/opds20/`（28 份，`$ref` 已改写成同目录文件名，可离线校验），
  生成脚本 `.workbuddy/verify/fetch_opds_schemas.py`。
- 规范给的三条 feed 硬要求：`metadata.title` 必填、`links` 里必须有 `rel=self`、
  至少含 `publications` / `navigation` / `groups` 之一。
- **Divina 侧**（章节路线的基础）：<https://readium.org/webpub-manifest/profiles/divina>。
  要点：声明合规要在 `metadata.conformsTo` 里含 `https://readium.org/webpub-manifest/profiles/divina`，
  媒体类型用 **`application/divina+json`**；`readingOrder` 里的每一项**必须**是位图，**建议**带 `width` / `height`；
  打包要么 `.divina` + `application/divina+zip`，要么直接走 CBZ/CBR；
  `toc` 的结构化 rel 有 `chapter` / `episode` / `issue` / `part` / `season` / `volume`；
  条漫用 `metadata.layout: "scrolled"`，日漫用 `metadata.readingProgression: "rtl"`。
- ⚠️ **OPDS 2.0 正文里没有出现 divina**（只说了"与 RWPM 基于同一抽象模型"）。所以
  `type: "application/divina+json"` 是合法的媒体类型字符串（link schema 对 `type` 不做枚举），
  但**规范没有为「OPDS feed → Divina 清单」规定专门的链接语义** —— 这条链路的实际可用性只能靠实测（§9.6）。
- 客户端：**Thorium Reader 是目前唯一被广泛验证支持 2.0 的阅读器**（第三方服务器项目 Storyteller 的
  实测表把它列为唯一一个跑通 2.0 的），FBReader 把 OPDS 2.0 标为 beta，KOReader 的 OPDS 浏览器是
  纯文本、只走 1.2，Panels / Chunky / Calibre / Moon+ / Aldiko 也都是 1.2。Thorium 本身支持 Divina，
  但「从 OPDS 2.0 feed 跟随 divina acquisition」这一步没有公开的验证记录。
- 结论：v2 面向的是「Readium 系客户端 + 自研脚本/工具」，不是既有用户的替代路径。**不值得为它牺牲 1.2 的兼容性。**

## 3. 边界（硬约束）

1. **1.2 的响应逐字节不变。** 验收方式见 §9.4：改动前先抓 19 条路由的基线，改动后逐条比对
   （`<updated>` 这类时间戳字段做遮罩）。
2. **不做内容协商。** 不在 `/api/opds/v1.2` 上按 `Accept: application/opds+json` 切到 v2，
   也不在 v2 上按 `Accept` 回落 1.2 —— 路径已经分版本，协商只会让 1.2 的行为变得不确定。
3. **不改共享文件的既有语义**：`xml.rs` / `model.rs` / `constants.rs` 不动；`repository.rs` 允许新增
   查询/字段，但 1.2 用到的既有方法与字段不得改名、改语义、改返回值。
4. **不动认证代码**（§1：前缀已覆盖）。清单端点也在 `/api/opds/` 下，自动同策略。
5. **不新增运行时依赖**：`serde`（已带 `derive`）、`serde_json`、`chrono` 都已在 workspace 与 crate 里。
6. **清单端点不需要新的源能力**：枚举页靠已有的 `SourceFetcher::fetch_pages`
   （`crates/suwayomi-domain/src/source.rs:39`，下载管理器在用）—— `chapter.page_count` 为 `-1`（未知）时用它补。
7. **清单端点的产物不是 OPDS feed**：它是 RWPM/Divina，校验走另一套 schema（§9.2），
   不进 §4 路由表以外的 OPDS 语义讨论。

## 4. 路由表（v2）

19 条里去掉 2 条、新增 1 条，共 18 条（**路径与 1.2 完全同形**，只换版本段）：

| v2 路径 | 对应 1.2 | v2 的形态 |
|---|---|---|
| `/` | `/` | `navigation`（根目录 9 项） |
| `/history` | 同 | `publications`（章节） |
| `/explore` | 同 | `navigation`（在线源） |
| `/explore/source/{source_id}` | 同 | `publications`（源里的漫画 = 远程条目；acquisition 指向源页面，见 §5.2） |
| `/library/series` | 同 | `publications`（漫画）+ `facets`（排序） |
| `/library/sources` | 同 | `navigation` |
| `/library/categories` | 同 | `navigation` |
| `/library/genres` | 同 | `navigation` |
| `/library/statuses` | 同 | `navigation` |
| `/library/languages` | 同 | `navigation` |
| `/library-updates` | 同 | `publications`（章节） |
| `/source/{source_id}` | 同 | `publications`（漫画） |
| `/category/{category_id}` | 同 | `publications`（漫画） |
| `/genre/{genre}` | 同 | `publications`（漫画） |
| `/status/{status_id}` | 同 | `publications`（漫画） |
| `/language/{lang_code}` | 同 | `publications`（漫画） |
| `/series/{series_id}/chapters` | 同 | `publications`（章节）+ `facets`（排序 / 过滤） |
| **`/series/{series_id}/chapter/{chapter_number}/manifest`** | **新增**（占 1.2 那个被去掉的 `/metadata` 的位置） | Readium Divina 清单，`application/divina+json`（§5.4） |
| ~~`/search`~~ | 1.2 返回 OpenSearch 描述文档 | **不提供**：2.0 的搜索是 feed 上的 templated link（§5.5） |
| ~~`/series/{id}/chapter/{n}/metadata`~~ | 1.2 的「章节元数据 feed」 | **不提供**：元数据已内联在章节 publication 上 |

query 参数与 1.2 同名同义（`lang` / `pageNumber` / `sort` / `filter` / `source_id` / `category_id` /
`status_id` / `lang_code` / `genre` / `query` / `author` / `title`）—— 客户端把 URL 里的版本段换掉就能用。

## 5. 逐字段映射（Atom → JSON）

### 5.1 feed 级

| 1.2（Atom） | v2（`metadata` / `links`） |
|---|---|
| `<id>urn:suwayomi:feed:…` | `metadata.identifier`（同一个 URN 原样搬；`feed-metadata.schema.json` 允许这个键，且 `urn:` 满足 `format: uri`） |
| `<title>` | `metadata.title`（必填） |
| `<updated>` | `metadata.modified`（`date-time`） |
| `<icon>` | 不映射 —— 2.0 的封面走 publication 的 `images`，feed 级没有图槽位 |
| `<author><name>Suwayomi` | **不映射**：Atom 要求 feed 必须有人类可读作者，2.0 的 feed metadata 没有对应要求（作者是 publication 的属性） |
| `opensearch:totalResults` | `metadata.numberOfItems`（≥ 0） |
| `opensearch:itemsPerPage` | `metadata.itemsPerPage`（**> 0**） |
| `opensearch:startIndex` | `metadata.currentPage`（**> 0**，1-based；1.2 那个是 0-based 下标，不要直接搬） |
| `<link rel="self">` | `links: [{rel: "self", href, type: "application/opds+json"}]`（必须有）。href 写成**无尾斜杠**的 `/api/opds/v2/<path>?<query>` —— 1.2 的 `{base}/?{q}` 写法取到的是 WebUI（§1） |
| `<link rel="start">` | 同 rel，指向 `/api/opds/v2?lang=…` |
| `<link rel="search">`（指向 OpenSearch 文档） | templated link（§5.5） |
| `first` / `previous` / `next` / `last` | 同 rel，只列**存在**的那几个（1.2 在 `total_pages > 1` 时才发，v2 照此） |
| `<link rel="facet" opds:facetGroup="sort">` | `facets: [{metadata: {title: "Sort"}, links: […]}]`，每组的 `links` 带 `title` 与 `properties.numberOfItems`；当前生效那项用 `rel: "self"` 标 |
| 无 | `groups` **不用**（根目录的 9 项用 `navigation` 就够，groups 只在"一个 feed 里要放多组"时才有意义） |

`navigation` 是链接数组：每条**必须**有 `title`（schema 的 `allOf` 强制），`rel` 用 1.2 的 `subsection`
（规范自己的 groups 示例里就是这个值）。

### 5.2 publication：漫画（`/library/series`、各 filter feed、explore 的远程条目）

| 1.2 | v2 `metadata` |
|---|---|
| `<id>urn:suwayomi:manga:{id}` | `identifier` |
| `<title>` | `title`（必填） |
| `<updated>` | `modified` |
| `<summary>`（"Status: … \| Source: … \| Language: …"） | **不搬**：这三段本来就各有着落（`publisher` / `language`），把它们拼成 description 是 1.2 为了凑 Atom 的 `summary` 才做的 |
| `<content>`（源上的简介） | `description` |
| `<author>` | `author`（RWPM 允许纯字符串，也可以写 `{name}`；只有要带 `role` / `links` 时才必须用对象） |
| `<category term label>`（题材） | `subject: ["Action", …]`（同上，纯字符串合法；带 `scheme` / `code` 时才用对象） |
| `<dc:publisher>`（= 源名） | `publisher`（同 `author` 的写法） |
| `<dc:language>` | `language`（**必须先过 BCP-47 正则**，见 §6.8） |
| `<dc:issued>` | `published` |
| `<link rel="alternate">`（源上的网页） | `links: [{rel: "alternate", type: "text/html"}]` —— **href 要按 `source.base_url` 展开成绝对 URL**，见下 |
| `<link rel="image">` / `"image/thumbnail"` | `images: [{href: 代理缩略图, type: "image/jpeg"}]` —— 2.0 的 `images` 不靠 rel 区分，两种合成一张即可 |
| `<link rel="subsection">`（指向章节 feed） | **改成 acquisition 链接**（§6.1），`type: application/opds+json` + `properties.indirectAcquisition` |

漫画的 acquisition 形态（`indirectAcquisition` 声明**经由章节 feed 最终拿到什么**）：

```json
{ "rel": "http://opds-spec.org/acquisition",
  "href": "/api/opds/v2/series/1/chapters?lang=en",
  "type": "application/opds+json",
  "title": "Chapters",
  "properties": { "numberOfItems": 42,
                  "indirectAcquisition": [{ "type": "application/divina+json" }] } }
```

**远程条目（`/explore/source/{id}`）是唯一一条没有间接获取可给的漫画**：它来自源的分页列表（`SManga`），
库里没有 id，因此没有章节 feed、也没有清单可以指向。1.2 给这种条目发的是 `rel="subsection"` +
**空 href**（`feeds.rs:802` 那句注释写着"readers rely on source browse only"）—— 也就是一条死链，
而且空串在 2.0 里也过不了 `format: uri-reference` 的意图。2026-10-01 定：**acquisition 指向源上的
漫画页**（`rel: http://opds-spec.org/acquisition` + `type: text/html` + `title: "Open on Source"`）。
它是这台服务器唯一能兑现的"获取"——客户端点开就是源站页面。其余字段照 §5.2 的漫画口径，
但没有 `belongsTo`（不属于任何库内作品）。

**源地址是"源内路径"，写进链接前必须展开成绝对 URL**（2026-10-01 实测）。`SManga::url` 与
`MangaAcqEntry::url` 存的是 `/g/450767/` 这种**源自己的地址**（只有 `real_url` 被填过才是绝对 URL）。
直接写进 `href` 会被解析成**本机**的路径 —— 打到 WebUI 的 SPA fallback 上。1.2 的每条
`rel="alternate"`（"View on Web"）都有这个毛病（实测基线里是 `href="/g/573225/"`）。
v2 里一律按 `source.base_url`（`source` 表本来就有这一列）展开：`https://nhentai.to` + `/g/573225/`
→ `https://nhentai.to/g/573225/`。落在 `v2/feeds.rs::source_page_url`。**1.2 一个字都不动**，
所以这是 v2 相对 1.2 的又一处有意差异（与 §5.1 的尾斜杠同类）。

### 5.3 publication：章节（`/series/{id}/chapters`、`/history`、`/library-updates`）

| 1.2 | v2 `metadata` |
|---|---|
| `<id>urn:suwayomi:chapter:{id}` | `identifier` |
| `<title>`（"Unread Chapter 3" 这种前缀） | `title`：**不带状态前缀**（2026-10-01 定），空名回落到 1.2 的 `chapter_title` 规则（`Oneshot` / `Chapter N`） |
| `<updated>` | `modified`（`date_upload`） |
| `<summary>`（`"{manga} — {chapter}" (Scanlator: X) — 5 of 20 pages read`） | `description`：**只承载 scanlator**，写成 `"{title} (Scanlator: X)"`，没有 scanlator 就整个键不写。1.2 拼进 summary 的另外三样各有归宿 —— 漫画名在 `belongsTo.series.name`、总页数在 `numberOfPages`、进度在链接 `properties` |
| `<author>`（→ 漫画作者） | `author` |
| `dc:*` 无 | `belongsTo: {series: {name: 漫画标题, position: 章节号}}` —— 章节挂回所属作品（`series` 对象要求 `name`） |
| 无 | `numberOfPages`（`page_count > 0` 时才写，schema 要求 > 0） |
| `<link rel="…pse/stream">`（取页模板） | **删掉**：PSE 是 1.2 的扩展位，2.0 走清单（§5.4） |
| `<link rel="…acquisition/open-access">`（已下载的 CBZ） | 保留（`downloaded` 时才给）。`rel` 用 IRI 形式（§6.2）；`type` 取 `opdsCbzMimetype`（§7）；`size` 可选 —— 数据层现在**没有**文件大小字段（1.2 的 `Link.length` 也一直没用上），要写就得先加查询，属可选增量 |
| 无 | **新增**：acquisition → 清单（§5.4） |
| `<link rel="image">`（章节封面 = 第 0 页） | `images: [{href: …/page/0, type: "image/jpeg"}]` |
| `<link rel="subsection">`（→ metadata feed） | 删掉（元数据已内联） |

章节的 acquisition 形态（**每个章节都有一条**）：

```json
{ "rel": "http://opds-spec.org/acquisition/open-access",
  "href": "/api/opds/v2/series/1/chapter/3/manifest",
  "type": "application/divina+json",
  "title": "Read",
  "properties": { "state": "in-progress", "readPage": 5, "readPages": 20,
                  "readAt": "2026-09-30T12:00:00Z" } }
```

`state` 沿用 1.2 `chapter_status`（`feeds.rs:313`）的判定优先级与含义，只是换成小写连字符：
`downloaded` → `in-progress`（`last_page_read > 0`）→ `unread`。
**状态与进度放链接的 `properties`**：它不是规范字段，但 link 的 `properties` 没有
`additionalProperties: false`（三份扩展 schema 都没有），所以能通过校验。

### 5.4 清单端点：Readium Divina（新增路由）

`GET /api/opds/v2/series/{series_id}/chapter/{chapter_id}/manifest` →
`Content-Type: application/divina+json`。

键用 **章节 id**，不用 1.2 那套 `source_order` —— 实测发现后者在一部作品里**不唯一**
（nhentai 系的扩展一卷一章，每行都写 `source_order = 0`）：库里 id 8 / 9 两条章节的
`source_order` 都是 0，1.2 的章节 feed 于是把两条 entry **都**指向同一个 metadata 子 feed
`/series/8/chapter/0/metadata`（feed 里根本没有 `chapter/1/metadata` 这条链接），
第二条章节的元数据取不到。清单是 v2 独有的路由，没有"与 1.2 同形"的包袱，用 id 才自洽
（`identifier` 本来就是 `urn:suwayomi:chapter:{id}`）。

`readingOrder` 里的取页链接仍写 `source_order` —— REST 层的口径是
`/api/v1/manga/{manga_id}/chapter/{source_order}/page/{n}`，不归本次改。实测（2026-10-01）：
库里 id 8 / 9 的 `chapter_number` **也**都是 `-1.0`，REST 那个"回落到 `chapter_number`"的兜底
同样分不开这两行，于是 `/api/v1/manga/8/chapter/0/page/0` 解析到未下载的 id 8 → 404；
**1.2 自己发的那条同形链接（`/library-updates` 里带 `pse:count="57"` 的）也 404** ——
属同一处既有数据缺陷，v2 没有变差。

```json
{ "@context": "http://readium.org/webpub-manifest/context.jsonld",
  "metadata": {
    "title": "Chapter 3",
    "identifier": "urn:suwayomi:chapter:42",
    "conformsTo": "https://readium.org/webpub-manifest/profiles/divina",
    "modified": "2026-09-30T12:00:00Z",
    "numberOfPages": 20,
    "author": "…",
    "belongsTo": { "series": { "name": "作品标题", "position": 3 } }
  },
  "links": [
    { "rel": "self",
      "href": "/api/opds/v2/series/1/chapter/3/manifest",
      "type": "application/divina+json" }
  ],
  "readingOrder": [
    { "href": "/api/v1/manga/1/chapter/3/page/0?updateProgress=true&opds=true", "type": "image/jpeg" },
    { "href": "/api/v1/manga/1/chapter/3/page/1?updateProgress=true&opds=true", "type": "image/jpeg" },
    { "href": "/api/v1/manga/1/chapter/3/page/19?updateProgress=true&opds=true", "type": "image/jpeg" }
  ] }
```

| 字段 | 来源 / 规则 |
|---|---|
| `@context` | 常量 `http://readium.org/webpub-manifest/context.jsonld` |
| `metadata.conformsTo` | 常量 `https://readium.org/webpub-manifest/profiles/divina`（**Divina 的合规声明就在这一个字段上**） |
| `metadata.title` / `identifier` / `modified` | 与章节 publication 同源（同一个 `chapter_title` 规则、同一个 `urn:suwayomi:chapter:{id}`、同一个 `date_upload`） |
| `metadata.numberOfPages` | `page_count > 0` 时写；未知则不写（schema 要求 > 0） |
| `metadata.author` | 与章节 publication 同源（漫画作者） |
| `metadata.language` | **不写** —— 章节与漫画两边的查询结果里都没有语言列（`ChapterListEntry` / `MangaDetails` 都没有 `source.lang`）。要写就得先加一条按 `manga.source` 取 `source.lang` 的查询，属可选增量，不在本次范围 |
| `metadata.belongsTo.series` | `{name: 漫画标题, position: 章节号}` |
| `links[].rel=self` | 清单自己的 URL，`type: application/divina+json` |
| `readingOrder[]` | **一页一条**，`href` 就是 1.2 用的那个取页路径（`/api/v1/manga/{manga_id}/chapter/{source_order}/page/{n}?updateProgress=true&opds=true`），`type: image/jpeg`。`updateProgress` 段按 `opdsEnablePageReadProgress` 决定（§7） |

**页的枚举**（清单路线唯一的硬骨头，阶段 2 已落地）：

1. `chapter.page_count > 0` → 直接 `0..page_count` 生成，**不会**再去问源；
2. `page_count == -1`（未知）→ 先 `repository::chapter_source_ref(chapter_id)` 取 `source` /
   `manga.url` / `chapter.url`（§3.6），再调 `SourceFetcher::fetch_pages(source_id, manga_url, chapter_url)`
   拿页列表，取**列表长度**当页数；
3. 拿不到 —— 章节没有源地址、源报错、源回了空列表，三者同等对待 → **502**，不返回空
   `readingOrder`（RWPM 的 `readingOrder` 是必填且空数组无意义）。三条失败都 `tracing::warn!`。

这一步**不回写** `chapter.page_count`：下载器回写是因为它本来就在改库，而清单是 GET ——
响应不该取决于它被请求过几次。

**刻意不写的字段**（写了就是编造）：

| 字段 | 为什么不写 |
|---|---|
| `metadata.readingProgression` | 数据层**没有**阅读方向（`manga` 表无对应列，§1）。默认 `ltr`，日漫想要 `rtl` 得先加列 —— 记为可选增量，不是本次范围 |
| `metadata.layout` | 条漫该写 `scrolled`，但源没有这个信息；不写取 divina 默认的固定版式 |
| `readingOrder[].width` / `height` | divina 是 **should** 不是 must；要知道尺寸得先把图取回来 |
| `toc` | 单章清单不需要目录；`chapter` 这类结构化 rel 是给整本/多卷用的 |

### 5.5 facets / 分页 / 搜索

- **facets**：1.2 是扁平的一堆 `<link rel="facet" opds:facetGroup="sort" opds:activeFacet>`；
  2.0 分组表达，`facets[].metadata.title` 是组名（"Sort" / "Filter"），组内每个链接自己带 `title`；
  当前生效的那项用 **`rel: "self"`** 表示（规范原文如此），不是 `activeFacet` 布尔。
  一组的 `links` 少于 2 条就不发这组（规范说 should 有两条以上，schema 只要求 ≥ 1）。
  每个链接带 `properties.numberOfItems`（`opds-properties.schema.json` 的链路属性）：排序组共享该 feed
  的总数（排序不改变结果集），过滤组按各自子集算 —— 所以章节 feed 的 Filter 组要一次取回两个计数。
- **分页**：`next` / `previous` / `first` / `last` 四个 rel 与 1.2 同名（1.2 用的就是 `previous`）。
  `previous` 与 `first` 在同一页时可以合并成一个链接的 `rel: ["first", "previous"]`（规范的分页示例就这么写）。
- **搜索**：不再有 OpenSearch 描述文档，改成每个 feed 上都挂一条模板链接：

```json
{ "rel": "search",
  "href": "/api/opds/v2/library/series{?query,title,author}",
  "type": "application/opds+json",
  "templated": true }
```

## 6. 规范硬约束（写错就会挂在校验器上）

以下每条都对着 `.workbuddy/verify/schemas/opds20/` 里的原文核过，不靠规范正文的印象。

1. **每个 publication 至少有一条 acquisition 链接**（`publication.schema.json` 的 `links.contains`）。
   → 漫画没有可下载的文件，用**间接获取**（`rel: acquisition` + 指向章节 feed + `properties.indirectAcquisition`）；
   章节一律给清单（`rel: http://opds-spec.org/acquisition/open-access`）—— 这两条覆盖所有条目，
   不存在"没有 acquisition 可给"的情况。**唯一的例外是 explore 的远程条目**（库里没有 id），
   它的 acquisition 指向源页面（§5.2）。
2. **acquisition 的 `rel` 枚举只有这些值**（`publication.schema.json` 的 `$defs.acquisition`）。
   ```
   acquisition / borrow / buy / preview / subscribe
   http://opds-spec.org/acquisition          http://opds-spec.org/acquisition/buy
   http://opds-spec.org/acquisition/open-access
   http://opds-spec.org/acquisition/borrow   http://opds-spec.org/acquisition/sample
   http://opds-spec.org/acquisition/subscribe
   ```
   **裸 `open-access`、`sample`、`download` 都不在枚举里** —— 只有 IRI 形式。1.2 的
   `REL_ACQUISITION_OPEN_ACCESS` 恰好是合法值，直接复用；但 `open-access` 这种简写会挂在 `contains` 上。
3. **结果为空时不能发 `publications: []`**：schema 是 `minItems: 1` + `anyOf[publications|navigation|groups]`。
   → 空结果改发 `navigation`（一条 `rel: "start"` 指回根目录、带 `title`）。**这是最容易漏的一条**：
   空库/无搜索结果时如果照常发 `publications: []`，响应就不合规。同理 `navigation` 与 `facets` 也是 `minItems: 1`
   —— 空数组要么换成另一种集合，要么整个键不写。
4. `navigation` 里每条链接**必须**有 `title`（feed schema 的 `allOf`）。
5. feed 的 `links`：`minItems: 1` + `uniqueItems: true` + 必须含一条 `rel: "self"`。
   `uniqueItems` 是**对象级**去重 → 两条完全相同的链接对象不合法（1.2 里同一个缩略图以 `image` 与
   `image/thumbnail` 两条出现，在 2.0 里合成 `images` 一条，正好回避）。
6. **feed 顶层不要放自定义键**：`additionalProperties` 指向 subcollection schema，未知键会被当成"子集合"去校验。
7. `metadata.title` 必填；`itemsPerPage` / `currentPage` 是 `exclusiveMinimum: 0`（未分页的 feed 别写
   `itemsPerPage`）；`numberOfItems` 是 `minimum: 0`（允许 0，与第 3 条不冲突：一个是计数、一个是数组长度）。
   publication 的 `numberOfPages` 同样是 `exclusiveMinimum: 0`（0 页的章节别写）。
8. `language` 必须匹配 **BCP-47 正则**（`metadata.schema.json` 的 `pattern`）。但**正则挡不住
   `all` / `other`** —— 这两个 Mihon 伪语言正好落在语法里（`all` 是 2-3 个 ASCII 字母、`other`
   是 5-8 个），语法合法、语义是错的。所以输出前要过两道：语法校验 **+** 显式排除
   `all` / `other` / 空值（落地在 `v2/json.rs::language_tag`）。凡是不通过的就不写这个键，
   不要回落成 `"en"` 之类的编造值。
9. `images` 一旦出现就 `minItems: 1`，且 `allOf.contains` 要求**至少一张**的 `type` 属于
   `image/jpeg|webp|avif|png|jxl|gif`。代理缩略图实际返回的类型由缓存里的 `.mime` 决定（可能 png/webp），
   1.2 一律声明 `image/jpeg`；v2 沿用同一口径 —— 记为**已知偏差**，不为了这一处去读 REST 的缓存布局。
10. `subject` / `author` / `publisher` / `belongsTo.series` 这几处，schema 的 `anyOf` **都允许纯字符串**；
    只有在要带 `role` / `scheme` / `code` / `position` 时才必须用对象，用对象时 `name` 必填。
11. `templated: true` 的链接，`href` 要过 `format: uri-template`；`templated` 不写（或 `false`）时
    同一字段按 `format: uri-reference` 校验，两者不能混（link schema 的 `if/then/else`）。
    本项目只有搜索链接是 templated。
12. **清单侧**（另一套 schema）：RWPM 要求 `metadata` + `readingOrder` 必填，且 `readingOrder` 每一项
    **必须带 `type`**；divina 的合规声明只在 `metadata.conformsTo` 上，媒体类型用 `application/divina+json`。
13. **feed 顶层不能写 `@context`**，哪怕规范正文的示例里出现过。`feed.schema.json` 把
    `additionalProperties` 指向 subcollection schema —— 那个 schema 的 object 分支要求
    `metadata` + `links`、array 分支要求元素是 Link 或子集合，一个字符串 `@context` 两个分支都不满足，
    直接挂在校验器上。清单侧相反：RWPM 的 `publication.schema.json` 明确列了 `@context`，**必须写**。

## 7. 设置项逐条处置（9 个 `opds*`）

2026-10-01 定：**v2 让它们生效**；1.2 一个都不跟（跟了就会改 1.2 的行为，违反 §3.1）。

| 设置 | 1.2 今天 | v2 |
|---|---|---|
| `opdsCbzMimetype` | 只在 REST 下载响应里生效（OPDS 侧写死 `TYPE_CBZ`） | ✅ 读它写 CBZ 链接的 `type` —— 这才是这个名字本来的用途 |
| `opdsItemsPerPage` | ❌ 常量 50 | ✅ `metadata.itemsPerPage` 与分页步长 |
| `opdsEnablePageReadProgress` | ❌ 恒 `updateProgress=true` | ✅ 决定清单里取页链接是否带 `updateProgress=true` |
| `opdsMarkAsReadOnDownload` | ❌ 恒 `markAsRead=true` | ✅ 决定 CBZ 链接是否带 `markAsRead=true` |
| `opdsChapterSortOrder` | ❌ 硬编码 `number_asc` | ✅ 作为章节 feed 的默认 `sort`（客户端显式给了 `sort` 就听客户端的） |
| `opdsShowOnlyDownloadedChapters` | ❌ 未生效 | ✅ 章节 feed 的隐含过滤（客户端没显式给 `filter` 时） |
| `opdsShowOnlyUnreadChapters` | ❌ 未生效 | ✅ 同上 |
| `opdsSkipChapterMetadataFeed` | ❌ 未生效 | **不适用**：2.0 内联元数据，没有 metadata 子 feed 可跳过 |
| `opdsUseBinaryFileSizes` | ❌ 未生效 | **不适用**：2.0 链接上的 `size` 定义就是**字节**，"KiB 还是 kB"只影响 1.2 的 `length` 展示口径 |

⚠️ 由此产生一个**已知的口径分叉**：同一设置在 v2 生效、在 1.2 不生效。接线做法照 `opds_cbz_mimetype` 走：
`ServerConfig` 加字段 → 默认值 → `apply_settings_blob` 加分支（GraphQL 侧的写入路径不动，
它写的本来就是同一个 blob）。1.2 是否跟进是**独立**的一次改动，需单独决定。

### 7.1 接线做法（阶段 5，2026-10-02 完成）

**链路**：`ServerConfig` 加 6 个字段（`opds_chapter_sort_order` 是新枚举 `ChapterSortOrder`，其余复用
已有类型）→ `apply_settings_blob` 认这 6 个 key → 设置页的**读**（`SettingsType::from_config`，原先这 6
项是硬编码常量，现在从 config 派生）与**写**（`setSettings` 的 `put!`，本来就写同一个 blob）都不需要改结构。
v2 侧把它们收进 `V2Ctx.config`（每请求一次 `config.snapshot()`），由 `V2Ctx` 上的
`items_per_page()` / `default_chapter_sort()` / `chapter_filter()` / `repo()` 分发给各 feed。

**默认值照参考实现**：`opdsItemsPerPage` = **50**、`opdsChapterSortOrder` = **DESC**（不是设置页原先
硬编码的 30 / Asc）。接通后有两处可见变化：设置页显示值变成 50 / DESC；**v2 章节 feed 的默认 `sort`
从 `number_asc` 变成 `number_desc`**（显式带 `sort=` 的客户端不受影响 —— 请求里的 `sort` 仍然优先）。

**两个 `show-only` 是叠加条件，不是默认 filter**：与参考实现的 `ChapterRepository` 一致 ——
`conditions.add(isDownloaded eq true)`，与客户端的 `filter` 参数**并列** AND。因此两者可以同时开
（取交集），`filter=unread` 与 `showOnlyDownloaded` 也取交集。facets 的 Filter 组计数跟着走
（`repository::chapter_counts` 也带这组条件）—— 这一条与参考实现**有意不同**：它的 facet 计数不带该条件，
但 1.2 的 facet 本来就不发计数，没有可对齐的行为。

**1.2 一个字没动**：页大小是 `OpdsRepository` 的字段，1.2 走 `OpdsRepository::new`（固定内置默认 50）；
`ChapterFilter::from_query` 只认 `unread`，1.2 的 `filter` 值域保持原样。`opdsItemsPerPage` 的
非正值在 v2 侧回落到默认（`itemsPerPage` 是 `exclusiveMinimum: 0`，且分页算术要拿它做除数）。

## 8. 实施分层

- **阶段 0 — 基线（2026-10-01 已完成）**：19 条 1.2 路由的响应快照已落在
  `.workbuddy/verify/out/opds12_baseline/pre/`（19 个 `.xml` + `manifest.json`），
  由 `.workbuddy/verify/opds12_baseline.py` 抓取，`opds12_regress.py` 做比对。抓完连抓两轮
  19/19 逐字节一致，快照可当基线用。跑法与遮罩口径见 §9.4。
- **阶段 1 — 骨架打样（2026-10-01 已完成）**：`src/v2/{mod,model,json,router,feeds}.rs`，挂载
  `.nest("/api/opds/v2", …)`。§6 的结构约束全部落进模型层（空结果走 `navigation`、acquisition 受
  枚举限制、`language` 语法 + 语义双检、空集合不序列化、feed 顶层无 `@context`）。
  路由 **4 条**：根 `/`、`/library/series`（含搜索分支）、`/series/{id}/chapters`、
  `/series/{id}/chapter/{n}/manifest`。清单端点提前到这一步，是因为章节的 acquisition 指向它 ——
  指向一个 404 等于 §6.1 只做了形式合规。清单目前只走 `page_count > 0` 的主路径。
- **阶段 2 — 清单端点的源侧兜底（2026-10-01 已完成）**：`page_count == -1` 时经
  `repository::chapter_source_ref` 调 `SourceFetcher::fetch_pages` 补页数（§5.4）；拿不到 → 502。
  集成测试用注入的桩源覆盖了三条路径（源给 N 页 / 源回空列表 / 源报错）。
  客户端侧见 §9.6。
- **阶段 3 — 补齐路由（2026-10-01 已完成）**：其余 14 条 —— 导航类 6 条（`/explore`、
  `/library/{sources,categories,genres,statuses,languages}`）共用一个 `navigation_feed` 组装器，
  每项的条目数写进链接的 `properties.numberOfItems`（feed 级 `numberOfItems` 是项数本身）；
  过滤类 5 条（`/source/{id}`、`/category/{id}`、`/genre/{g}`、`/status/{id}`、`/language/{code}`）
  都是给 `library_series_feed` 固定一个维度、其余留给 query，路由形状与 1.2 逐条对应；
  `/explore/source/{id}` 走远程条目（acquisition 指向源页面，§5.2）；`/history` 与 `/library-updates`
  发章节 publication。**18 条路由齐了**（19 条 1.2 去掉 `/search` 与 `/metadata`、加上清单）。
  章节 publication 在三个 feed 里共用一套：漫画名**不**拼进标题（1.2 的 `add_manga_title`），
  它在 `belongsTo.series.name` —— 与 §5.2 的"每个事实只有一个归宿"同一口径。
- **阶段 4 — facets 的计数（2026-10-01 已完成）**：分组、组名、"当前项标 `rel: self`"与四个分页 rel
  都在阶段 1 落地了（`sort_facets` / `chapter_facets` / `pagination_links`）；这一步补上 §5.1 的
  `properties.numberOfItems` —— 排序组共享该 feed 的总数，章节的 Filter 组两个选项各算一次
  （`repository::chapter_counts` 用一条 SQL 同时取回全部与未读，不然就得把章节列表查两遍）。
- **阶段 5 — 设置接线（2026-10-02 已完成）**：按 §7 的表接 6 项（其余 3 项不适用/不需要）。
  core 加字段与 blob 分支、设置页的显示值改为从 config 派生、v2 经 `V2Ctx.config` 读取；
  两个 `show-only` 按参考实现做成**与 `filter` 并列的叠加条件**。做法与两处行为变化见 §7.1。
- **阶段 6 — 文档与客户端（2026-10-03 已完成，只差 Thorium 实测）**：`rest-api.md` 加 §6 一节
  （18 条路由；写明 v2 是**本仓新增**、参考实现没有，所以那一节不是兼容对照）、`user-guide.md` 的
  OPDS 一节改成 1.2 / 2.0 双端点对照（定位、客户端现状、设置只在 v2 生效）、两份 README 去掉
  `In-progress`；`auth.rs` 单测与 `.workbuddy/verify/auth_matrix.py` 各补 v2 路径用例（§9.5）。
  **§9.6 的 Thorium Reader 人工实测仍未跑** —— 本机没装、且它是 GUI 程序，现有工具驱动不了。

## 9. 验证

1. **crate 集成测试（阶段 5 已完成）**：`crates/suwayomi-opds/tests/opds_v2.rs`（29 项）+ `src/v2/json.rs`
   里的 BCP-47 单测（4 项），照 `tests/opds_feeds.rs` 的模子（内存 SQLite 播种 + 断言）。断言方式用
   `serde_json::to_value` 后按路径取值，**不要**用字符串 `contains`（JSON 的键序不保证）。已覆盖：
   根 feed 是 navigation 且每条 link 都带 `title`、空结果走 `navigation` 而不是 `publications: []`、
   每个 publication 都有 acquisition、`all` 伪语言被丢弃、章节的 `belongsTo` / `state` / 标题无状态前缀、
   集合去重、**清单的 `readingOrder` 长度等于页数且每项带 `type`**、`page_count == -1` 时不给清单、
   `source_order` 重复（两章同为 0）时清单仍按**章节 id** 区分、不存在的作品回 `NotFound`、
   **`fetch_pages` 兜底**（源给 N 页 → `numberOfPages` 与 `readingOrder` 都是 N，且已知页数的章节不被覆盖）、
   源回空列表与源报错都回 `PageCountUnknown`、
   **阶段 3 新增**：导航 feed 的 `href` / `rel` / `properties.numberOfItems` 与空导航回落根目录、
   远程条目的 acquisition 指向源页面（展开成绝对 URL 的 `text/html`，且不带 `belongsTo`）、
   **源内路径按 `source.base_url` 展开**（远程条目与库内作品的 `alternate` 各一条断言）、
   `sort=latest` 只在该源 `supports_latest` 时走 latest（否则回落 popular 且标题仍跟请求）、
   `/history` 只列读过的章节且标题不带漫画名、`/library-updates` 列全部章节、
   过滤 feed 的标题与 `rel=self`（含 1.2 那条冗余的 `source_id=` query，属有意的形状对齐）、
   **阶段 4 新增**：排序组的每项都带同一个 `numberOfItems` 且该数跟着 feed 被固定的那个维度走
   （库内 2 部、`/source/1` 命中 1 部）、章节 Filter 组的两项各报自己的计数（全部 3 / 未读 2）且
   当前项仍是 `rel: self`、
   **阶段 5 新增**：`opdsItemsPerPage` 同时决定 `metadata.itemsPerPage` 与分页步长（3 章按每页 2 条
   分成两页、第 2 页 1 条且带 `next`）、非正值回落默认、`opdsChapterSortOrder` 映射到默认 `sort` 键、
   `opdsMarkAsReadOnDownload` 与 `opdsCbzMimetype` 改 CBZ 链接的 `markAsRead=` 与 `type`、
   `opdsEnablePageReadProgress` 改清单取页链接的 `updateProgress=`、
   两个 `show-only` 各自收窄章节 feed 且**同时开取交集**（`filter=unread` 与 `onlyDownloaded` 也取交集），
   facet 的 Filter 组计数跟着收窄后的集合走。

   注入桩源要绕一道弯：`SourceBackend::Test` 带 `#[cfg(test)]`，而 `cfg(test)` **不传播到依赖 crate**，
   所以 `tests/*.rs` 里构造不出来 —— 靠 `suwayomi-domain` 的 `test-util` feature 把它放进构建
   （`suwayomi-opds` 在 `[dev-dependencies]` 里开，生产构建不开，枚举仍是封闭两态）。
   另外桩要 `impl SourceFetcher`，而那个 trait 是 `#[async_trait]` 声明的，测试目标需要 `async-trait`
   这个 dev-dependency。
2. **JSON Schema 校验（阶段 1 已落地）**：离线 schema 集在 `.workbuddy/verify/schemas/opds20/`
   （28 份、173 个 `$ref` 零悬空），由 `.workbuddy/verify/fetch_opds_schemas.py` 重新生成。
   校验器 `.workbuddy/verify/opds_v2_schema_check.py`：每个 feed 响应跑 `feed.schema.json`，
   清单跑 `rwpm-publication.schema.json` + 检查 `conformsTo` 含 divina 的 URI。`jsonschema` 在隔离
   venv `C:\Users\16695\.workbuddy\binaries\python\envs\default`（4.26.0）。
   两个必须先知道的坑：
   - 两份 schema 里 `language` 的 pattern 用 **.NET 风格的命名组** `(?<name>…)`，Python 的 `re`
     编不过（同一 pattern 里还有重名组，改成 `(?P<name>…)` 也仍然失败）→ 加载时统一降级成非捕获组
     `(?:`。命名组只影响捕获、不影响匹配语义。
   - 那条例行 pattern **接受 `all` / `other`**（2-3 / 5-8 个 ASCII 字母，正好落在语法里）。
     所以"schema 通过"不等于"语言合法" —— §6.8 的语义排除只能在代码里做。
   脚本带 `--selftest`（12 例正反例：空 `publications`、缺 `title` 的 navigation 链接、
   顶层 `@context`、裸 `open-access`、无 acquisition 的 publication，以及 `language` 的三态）——
   **先证明校验器本身可信，再拿它判别人**。
3. **端到端**：按项目惯例**替换 Suwayomi-latest 的产物**（不另起端口，端口从运行态嗅探，
   通常 4567），逐条拉取 18 条 v2 路由，断言 200 + `Content-Type: application/opds+json`
   （清单是 `application/divina+json`）+ schema 通过；导航类与列表类各挑一条人工看一眼 JSON。
   另有一条**按客户端方式走链路**的脚本 `.workbuddy/verify/opds_v2_client_walk.py`：只跟随 feed
   里的链接（除目录根外不硬编码路径），走 根 → `Library` → 作品 → 章节 feed → 章节 → 清单 →
   `readingOrder` 取页 → 清单 `rel=self` 自指；任一环坏掉就 exit 1。它验的是"客户端能跟随的
   每一环都能解析"，验不了"某个具体客户端愿意跟"（那要看第 6 条）。
4. **1.2 回归（本次的验收重点）**：改用带 v2 的产物起一次实例，重抓一轮再与基线比对：

   ```
   python .workbuddy/verify/run_latest_server.py --root <隔离根> --port 4567   # 后台起
   python .workbuddy/verify/opds12_baseline.py --out <同上> --label post
   python .workbuddy/verify/opds12_regress.py \
       --base .workbuddy/verify/out/opds12_baseline/pre \
       --head .workbuddy/verify/out/opds12_baseline/post     # 退出码 0 才算过
   ```

   **任何一处差异都视为失败** —— 这是"不影响 1.2"的唯一硬证据，不靠"我只加了新文件"的推理。
   两条前提：比对两侧要用**同一个库**（基线绑定抓取时刻的库内容，库变了会假阳性），
   且路径要写**无尾斜杠**的 `/api/opds/v1.2`（见 §1 的实测补充）。动态字段的遮罩口径：只有落在
   抓取时间窗内的 `<updated>` 换成 `{{NOW}}` —— feed 级与部分 entry 走 `now_opds()`（当前时间），
   另一批 entry 走 `epoch_opds()`（来自库，稳定），两者格式相同，只能按时间窗区分。
   `pse:lastReadDate` 来自库、稳定，不遮罩。
5. **认证**：`/api/opds/v2/**` 在 `UI_LOGIN` 模式下匿名 401、`?token=` 放行。2026-10-03 已补用例：
   `crates/suwayomi-api/src/auth.rs` 的 `opds_v2_falls_under_the_same_auth_prefixes`（两条前缀判定）
   与 `token_query_is_limited_to_opds_and_pages` 多一行 v2 断言；`.workbuddy/verify/auth_matrix.py`
   的数据接口匿名 401 清单加了 v2 三条（根、`library/series`、清单）、`?token=` 段加一条。
   清单里的取页链接不必另想办法：它指向 `/api/v1/manga/{id}/chapter/{n}/page/{i}`，本来就在
   `?token=` 的白名单里，与 1.2 的取页链接同一条路径族。
6. **客户端**：Thorium Reader 手工加目录（浏览 → 打开作品 → 章节列表 → **打开清单读到页**）。
   这一条是本次最大的未知：Thorium 支持 Divina，但"从 OPDS 2.0 feed 跟随 divina acquisition"没有公开的
   验证记录。**到阶段 6 收尾（2026-10-03）仍未跑**：这台机器上没装 Thorium Reader，而它是 GUI 程序，
   现有工具驱动不了。替代品是第 3 条那个链路走查 —— 它证明客户端要跟随的每一环都能解析，
   但**不**证明 Thorium 真的会跟 `indirectAcquisition` 走。这一条仍需人工（装 Thorium → 填目录 URL →
   点开作品 → 点开章节 → 翻页）—— 它是本方案唯一未验的假设。走不通也不影响交付：v2 的章节仍可作为
   纯 CBZ 入口（下载型客户端），只是"在线阅读"要另想办法。KOReader 用 1.2 做对照组，确认两条端点互不影响。

## 10. 已定（2026-10-01）

| # | 事项 | 结论 |
|---|---|---|
| 1 | 路径 | 保持 **`/api/opds/v2`**（与两份 README 的写法一致；认证前缀也正好覆盖） |
| 2 | 章节的表达 | 走**标准**路线：Readium Divina 清单（§5.4），不用 1.2 的 PSE 扩展位 |
| 3 | 设置分叉 | **让 §7 的 6 项在 v2 生效**，1.2 不跟（跟了就是改 1.2） |
| 4 | 状态前缀 | **不保留**在标题里；状态与进度进链接的 `properties`（§5.3） |
| 5 | 做到哪一层 | **阶段 0–6 全做**（不含 §11 明确不做的事） |

由上表派生的新待决（都不是本次阻塞项）：

- `readingProgression` 要不要支持：数据层没有阅读方向列（§5.4）。要支持得先加列 + 迁移 + 让源侧填，
  属独立改动。
- 1.2 是否跟进那 6 个设置：独立改动，需单独决定（§7）。

## 11. 不做的事

- **OPDS Authentication 1.0**（`/auth` 文档、OAuth 流程）：本仓的凭据模型是 Basic / cookie / `?token=`，
  与 1.2 同源。要做是独立计划（会影响认证层，超出"不影响 1.2"的范围）。
- **LCP / 价格 / 借阅**（`price` / `indirectAcquisition` 里的付费链路 / `holds` / `copies`）：
  没有这项业务；`indirectAcquisition` 只用来表达"经由章节 feed 取到清单"。
- **内容协商**（§3.2）。
- **`groups`**：只有"一个 feed 里要放多组"时才需要，当前路由没有一个符合。
- **整本漫画的单一 Divina 清单**（`toc` 列出全部章节）：要枚举每章的全部页，一次请求得打源 N 次；
  阅读器本来就是一章一章读的，没有这个需求。
- **PSE 扩展位**（`http://vaemendis.net/opds-pse/stream`）：1.2 的兼容手段，v2 走标准清单（§10.2）。
- **替换或改造 1.2**：1.2 是本项目全部既有 OPDS 客户端的唯一入口，本次及后续都不动它的语义。
