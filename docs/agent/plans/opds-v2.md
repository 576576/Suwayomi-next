# OPDS 2.0（`/api/opds/v2`）方案与计划

给**维护者与 AI** 看：为什么要做、与 1.2 的硬边界、逐字段映射、分阶段落地、怎么验。
现状那一节（§1）是动手前必读的部分。五项选型已于 **2026-10-01 定稿**（§10）。

## 0. 结论先行

- 新端点 **`/api/opds/v2`**，与 `/api/opds/v1.2` **并列挂载**（`crates/suwayomi-server/src/lib.rs:160` 的 `build_router`）。两份 README 里已经写着
  `OPDS: /api/opds/v1.2（In-progress: /api/opds/v2）`。
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

文档面（改动时同步）：

- `docs/agent/rest-api.md` §3 的 19 行路由表；
- `docs/zh/user-guide.md` 「OPDS / KOReader」一节；
- 根 `README.md` 与 `docs/en/README.md` 各一行 `In-progress: /api/opds/v2`。

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
| `/explore/source/{source_id}` | 同 | `publications`（源里的漫画 = 远程条目） |
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
| `<link rel="alternate">`（源上的网页） | `links: [{rel: "alternate", type: "text/html"}]` |
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

### 5.3 publication：章节（`/series/{id}/chapters`、`/history`、`/library-updates`）

| 1.2 | v2 `metadata` |
|---|---|
| `<id>urn:suwayomi:chapter:{id}` | `identifier` |
| `<title>`（"Unread Chapter 3" 这种前缀） | `title`：**不带状态前缀**（2026-10-01 定），空名回落到 1.2 的 `chapter_title` 规则（`Oneshot` / `Chapter N`） |
| `<updated>` | `modified`（`date_upload`） |
| `<summary>`（"… — 5 of 20 pages read"） | `description`（同文本；读数进度不在这里，另进取页链接的 `properties`） |
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

`GET /api/opds/v2/series/{series_id}/chapter/{chapter_number}/manifest` →
`Content-Type: application/divina+json`。

```json
{ "@context": "http://readium.org/webpub-manifest/context.jsonld",
  "metadata": {
    "title": "Chapter 3",
    "identifier": "urn:suwayomi:chapter:42",
    "conformsTo": "https://readium.org/webpub-manifest/profiles/divina",
    "modified": "2026-09-30T12:00:00Z",
    "numberOfPages": 20,
    "author": "…",
    "language": "en",
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
| `metadata.author` / `language` | 与章节 publication 同源（`language` 同样要先过 BCP-47，§6.8） |
| `metadata.belongsTo.series` | `{name: 漫画标题, position: 章节号}` |
| `links[].rel=self` | 清单自己的 URL，`type: application/divina+json` |
| `readingOrder[]` | **一页一条**，`href` 就是 1.2 用的那个取页路径（`/api/v1/manga/{manga_id}/chapter/{source_order}/page/{n}?updateProgress=true&opds=true`），`type: image/jpeg`。`updateProgress` 段按 `opdsEnablePageReadProgress` 决定（§7） |

**页的枚举**（清单路线唯一的硬骨头）：

1. `chapter.page_count > 0` → 直接 `0..page_count` 生成；
2. `page_count == -1`（未知）→ 调 `SourceFetcher::fetch_pages(source_id, manga_url, chapter_url)`
   取页数（§3.6）。`repository.rs` 需要新增一条按 `chapter.id` 取 `source` / `manga.url` / `chapter.url` 的查询；
3. 两者都拿不到（源不可用）→ 返回 **502/503**，不要返回一份空 `readingOrder`
   （RWPM 的 `readingOrder` 是必填且空数组无意义）。

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
   不存在"没有 acquisition 可给"的情况。
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
8. `language` 必须匹配 **BCP-47 正则**。源表里的 `lang` 可能是 `all` / `other` / 空值
   （Mihon 系扩展的语言枚举里就有这两个），**不能原样写进去** → 只在通过校验时输出 `language`。
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

## 8. 实施分层

- **阶段 0 — 基线（2026-10-01 已完成）**：19 条 1.2 路由的响应快照已落在
  `.workbuddy/verify/out/opds12_baseline/pre/`（19 个 `.xml` + `manifest.json`），
  由 `.workbuddy/verify/opds12_baseline.py` 抓取，`opds12_regress.py` 做比对。抓完连抓两轮
  19/19 逐字节一致，快照可当基线用。跑法与遮罩口径见 §9.4。
- **阶段 1 — 骨架打样**：`src/v2/{mod,model,json,router,feeds}.rs`；先实现 **3 条**：根 `/`、
  `/library/series`、`/series/{id}/chapters`。挂载 `.nest("/api/opds/v2", …)`。
  这一步要同时落地 §6 的全部约束（尤其空结果与 acquisition 链接），因为它们是模型层的形状，
  补在后面等于重写。
- **阶段 2 — 清单端点（Divina）**：§5.4 的路由 + 页枚举（含 `fetch_pages` 兜底）+ 章节的 acquisition 链接。
  排在前面是因为它是本次唯一的**新机制**，风险集中在这里（§9.6 的客户端链路）。
- **阶段 3 — 补齐路由**：其余 14 条 —— 导航类 6 条（`/explore`、`/library/{sources,categories,genres,statuses,languages}`）、
  过滤类 5 条（`/source/{id}`、`/category/{id}`、`/genre/{g}`、`/status/{id}`、`/language/{code}`）、
  `/explore/source/{id}`、`/history` 与 `/library-updates`。
- **阶段 4 — facets 与分页**：把 1.2 的 `sort` / `filter` facet 组翻译成 2.0 的 `facets` 结构。
- **阶段 5 — 设置接线**：按 §7 的表接 6 项（其余 3 项不适用/不需要）。
- **阶段 6 — 文档与客户端**：`rest-api.md` 加 v2 一节、`user-guide.md` 改 OPDS 一节（写清 v2 的定位与
  客户端现状）、两份 README 去掉 `In-progress`；手工用 Thorium Reader 加一次目录，跑通
  "浏览 → 打开作品 → 看章节 → **打开清单并翻页**"。

## 9. 验证

1. **crate 集成测试**：`crates/suwayomi-opds/tests/opds_v2_feeds.rs`，照 `tests/opds_feeds.rs` 的模子
   （内存 SQLite 播种 + 断言）。断言方式用 `serde_json::from_str::<Value>` 后按路径取值，
   **不要**用字符串 `contains`（JSON 的键序不保证）。覆盖：三类 feed 的形状、空结果走 `navigation`、
   每个 publication 都有 acquisition、`language` 过滤、章节的 `belongsTo`、**清单的 `readingOrder` 长度等于页数**、
   `page_count == -1` 时走 `fetch_pages`（用 `SourceBackend::Test` 注入桩，见 `download.rs` 的 `PageListStub` 用法）。
2. **JSON Schema 校验**：离线 schema 集在 `.workbuddy/verify/schemas/opds20/`（28 份，`$ref` 已本地化），
   由 `.workbuddy/verify/fetch_opds_schemas.py` 重新生成。验证脚本对**每个路由的响应体**跑
   `feed.schema.json`；对**清单响应**跑 `rwpm-publication.schema.json` + 检查 `conformsTo` 含 divina 的 URI。
   需要 `jsonschema`（本机 miniconda 里没有）→ 装进隔离 venv
   `C:\Users\16695\.workbuddy\binaries\python\envs\default`，不要装到系统 Python。
3. **端到端**：按项目惯例**替换 Suwayomi-latest 的产物**（不另起端口，端口从运行态嗅探，
   通常 4567），逐条拉取 18 条 v2 路由，断言 200 + `Content-Type: application/opds+json`
   （清单是 `application/divina+json`）+ schema 通过；导航类与列表类各挑一条人工看一眼 JSON。
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
5. **认证**：`/api/opds/v2/**` 在 `UI_LOGIN` 模式下匿名 401、`?token=` 放行（既有断言在
   `crates/suwayomi-api/src/auth.rs` 的单测与 `.workbuddy/verify/auth_matrix.py`，补一条 v2 路径的用例）。
   清单里的取页链接同样受保护 —— 客户端是带 Basic 头抓图，还是 `?token=`，要在第 6 步一并看。
6. **客户端**：Thorium Reader 手工加目录（浏览 → 打开作品 → 章节列表 → **打开清单读到页**）。
   这一条是本次最大的未知：Thorium 支持 Divina，但"从 OPDS 2.0 feed 跟随 divina acquisition"没有公开的
   验证记录。若这条路走不通，v2 的章节仍可作为纯 CBZ 入口（下载型客户端），但"在线阅读"要另想办法 ——
   **阶段 2 结束就测，不要等到阶段 6**。KOReader 用 1.2 做对照组，确认两条端点互不影响。

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
