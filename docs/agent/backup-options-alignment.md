# 创建备份的可选项：对齐 Mihon

目标：把「创建备份」对话框的复选框集合与语义对齐 Mihon，包括新增
**所有已读作品 / 插件商店 / 图源设置 / 包含敏感设置**，并把现有的
**服务设置 + 客户端数据** 合成一个「应用设置」。

核对基准：

| 上游 | commit | 关键文件 |
| --- | --- | --- |
| Mihon | `866d045` | `app/src/main/java/eu/kanade/tachiyomi/data/backup/create/BackupOptions.kt` |
| 上游 Suwayomi | `dba836b` | `server/server-config/src/main/kotlin/suwayomi/tachidesk/manga/impl/backup/BackupFlags.kt` |

术语约定：下文**上游 Suwayomi**（或简称「上游」）= `Suwayomi/Suwayomi-Server`，是本仓 fork 的来源，
当前的 `PartialBackupFlagsInput` 七字段就照抄自它；**Mihon** = `mihonapp/mihon`，
是备份格式（`.tachibk` 的 proto）与选项集的权威来源。两者对「选项」的定义并不相同。

---

## 0. 已定选型（2026-09-29）

| 项 | 选定 | 落地位置 |
| --- | --- | --- |
| 图源设置 | 改沙盒拿扁平 key/value，tag 105 逐字对齐 Mihon | `ext-runtime` 加接口 + L3 |
| 应用设置 | 新增 `includeAppSettings`；旧 `includeClientData` / `includeServerSettings` 废弃 | L2 / L5 |
| 包含敏感设置 | **默认关闭**，管 9002 tracker 凭据 + 9001 认证用户名/密码 | L2 / L3 |
| 所有已读作品 | 已定：导出侧照 Mihon 查「非库已读」；恢复侧 `in_library` 跟随 `favorite` | L3 / L4 |

---

## 1. 上游 Mihon 的完整选项集

`BackupOptions`（`BackupOptions.kt:6`）共 10 项，分两组
（`libraryOptions` / `settingsOptions`），默认值与**灰化依赖**如下：

| # | 字段 | 文案（`i18n/.../base/strings.xml`） | 默认 | 灰化条件（`enabled`） |
| --- | --- | --- | --- | --- |
| 1 | `libraryEntries` | Library entries | true | — |
| 2 | `categories` | Categories | true | — |
| 3 | `chapters` | Chapters | true | `!libraryEntries` |
| 4 | `tracking` | Tracking | true | `!libraryEntries` |
| 5 | `history` | History | true | `!libraryEntries` |
| 6 | `readEntries` | **All read entries**（`non_library_settings:617`） | true | `!libraryEntries` |
| 7 | `appSettings` | **App settings**（`:614`） | true | — |
| 8 | `extensionStores` | Extension repos | true | — |
| 9 | `sourceSettings` | **Source settings**（`:615`） | true | — |
| 10 | `privateSettings` | **Include sensitive settings (e.g., tracker login tokens)**（`:616`） | **false** | `!appSettings && !sourceSettings` |

另有两条 UI 规则需要一并对齐（`CreateBackupScreen.kt:76`、`BackupOptions.kt:32`）：

- `canCreate()` = `libraryEntries || categories || appSettings || extensionStores || sourceSettings`
  —— 全关时「创建」按钮禁用（注意 `chapters`/`tracking`/`history`/`readEntries` **不**参与判定）。
- `privateSettings` 是**唯一默认关闭**的项。

Mihon 顶层的 proto 结构（`models/Backup.kt`），本仓缺三个 tag：

| tag | 字段 | 对应选项 |
| --- | --- | --- |
| 1 / 2 / 101 | `backupManga` / `backupCategories` / `backupSources` | 本仓已有 |
| **104** | `backupPreferences` | `appSettings` |
| **105** | `backupSourcePreferences` | `sourceSettings` |
| **106** | `backupExtensionStores` | `extensionStores` |

子消息号（`models/BackupPreference.kt`、`models/BackupExtensionStore.kt`）：

- `BackupPreference`: 1 `key`, 2 `value`（`PreferenceValue` 是 sealed，实际是 oneof）
- `BackupSourcePreferences`: 1 `sourceKey`, 2 `prefs`
- `BackupExtensionStore`: 1 `indexUrl`, 2 `name`, 3 `badgeLabel`, 4 `contactWebsite`,
  5 `signingKey`, 6 `contactDiscord`, 7 `isLegacy`, 8 `extensionListUrl`

`privateSettings` 的过滤规则不是字段、而是 key 前缀（`core/common/.../Preference.kt:29`）：
`__PRIVATE_` 开头的键被剔除，`__APP_STATE_` 开头的键永不入备份。

---

## 2. 实施前的本仓现状（2026-09-29）

> 本节是动手前的快照，用于说明改动量；落地情况见 §7。

GraphQL `PartialBackupFlagsInput`（`crates/suwayomi-graphql/src/mutation_b4.rs:372`）7 项，
与上游 Suwayomi 逐字一致。**但导出侧只有 4 项真正生效**
（`crates/suwayomi-core/src/backup.rs::build_backup`）：

| 本仓 flag | 导出侧实际效果 | 依据 |
| --- | --- | --- |
| `includeManga` | ✅ `SELECT * FROM manga WHERE in_library = TRUE` | `backup.rs:835` |
| `includeCategories` | ✅ | `backup.rs:876` |
| `includeChapters` | ✅ | `backup.rs:843` |
| `includeTracking` | ✅ 且连带 `tracker_credential`（9002） | `backup.rs:886`、`:945` |
| `includeHistory` | ❌ 空操作 | — |
| `includeClientData` | ❌ 空操作（`meta` 恒为空 map） | `backup.rs:965` |
| `includeServerSettings` | ❌ 空操作（`server_settings` 恒为 `None`） | `backup.rs:966` |

`backup.rs:22-24` 的注释已声明前三个是空操作。

WebUI 侧（`Suwayomi-WebUI`）已有分组与隐藏机制：
`BACKUP_FLAGS_BY_GROUP`（`Backup.constants.ts:31`）两组、
`BackupFlagInclusionDialog` 的 `hiddenFlags` prop、`canCreate` 尚无对应实现。
`Backup.utils.ts:18/28` 的两个映射与 `autoBackupInclude*` 服务端设置 1:1 对应。

---

## 3. 逐项对齐方案

### 3.1 所有已读作品（`readEntries`）

**先说概念：本仓只有一个"在不在书库里"的布尔量，就是 `manga.in_library`。**

| 本仓 | Mihon | 含义 |
| --- | --- | --- |
| `manga.in_library: bool` | `Manga.favorite: bool`（派生自 `favoriteAt != null`，`domain/.../Manga.kt:43`） | 该作品是否属于「书库」——是则出现在书库列表、参与更新，否则只是一个有记录的作品 |
| `BackupManga.favorite`（proto tag 100，`backup.rs:238`） | 同名字段（`BackupManga.kt:35`，默认 `true`） | 上面这个布尔量在备份文件里的载体 |

本仓没有第二个类似「收藏 / 关注」的概念，所以**「入库」「入栏」说的是同一件事**
（`in_library = TRUE`）；此前的写法不统一，以下一律用「入库」。

Mihon 的导出（`BackupCreator.kt:85`）：

```kotlin
val nonFavoriteManga = if (options.readEntries) mangaRepository.getReadMangaNotInLibrary() else emptyList()
val backupManga = backupMangas(getFavorites.await() + nonFavoriteManga, options)
```

`getReadMangaNotInLibrary()` 返回的是**有已读记录但不在书库**的作品，它们以 `favorite = false`
混在同一个 `backupManga` 列表里。本仓的数据源与它等价：

```sql
SELECT * FROM manga
WHERE in_library = FALSE
  AND EXISTS (SELECT 1 FROM chapter c WHERE c.manga = manga.id AND c.read = TRUE)
```

**卡点在恢复侧，不在导出侧。** `backup.rs:551-556` 把 `in_library` 硬编码为 `TRUE`，
且注释写着「`BackupManga.favorite` is NOT "in library"」——这条断言与 Mihon 的实际语义相反：
恢复时 `favoriteAt` 正是由 `BackupManga.favorite` 决定（`BackupManga.kt:63`，
`favorite = false` → `favoriteAt = null` → `Manga.favorite = false`），
**favorite 就是在库**。

后果：恢复一份 Mihon 备份（其 `readEntries` 默认开启、必然带 `favorite = false` 条目）时，
这些作品会被本仓全部入库，与 Mihon 自己的恢复结果不一致。这也说明：
不修这条，`readEntries` 的往返不可能自洽（导出 100 条 → 再导入变成 100 条全在库）。

### 3.2 插件商店（`extensionStores`）

数据源现成：`extension_store` 表（`index_url, name, badge_label, signing_key, contact_website`）。
可映射 tag 1/2/3/5/4；Mihon 的 6 `contactDiscord`、7 `isLegacy`、8 `extensionListUrl`
本仓没有对应列，留空即可（恢复端视 `null` 为「无」）。**无需决策，可直接做。**

### 3.3 图源设置（`sourceSettings`）

Mihon 落 105（`sourceKey` + 扁平 `key/value` 列表），`sourceKey` = `source.preferenceKey()`。
上游 Suwayomi 的实现是 `"source_$id"`（`eu/kanade/tachiyomi/source/ConfigurableSource.kt:21`）。

本仓**没有服务端的图源设置存储**：`SandboxClient::source_preferences`
（`crates/suwayomi-domain/src/source/sandbox.rs:209`）只是向沙盒要
「设置界面的 JSON 数组」（`PreferenceScreen` 序列化结果），值由扩展自己在 JVM 里保存。
沙盒只有两个接口：取界面 JSON、按 `position` 写回单值；扁平 key/value 是第三个（已落地，见 §7）。

**已选：方案 A（改沙盒拿扁平 key/value）。** 需要动 `Suwayomi-ext-runtime`（独立仓库，Kotlin 侧）：

1. 沙盒新增 `GET /source/{id}/preferences/raw`，返回 `SharedPreferences.all` 的扁平 key/value；
2. 服务端为每个 `ConfigurableSource` 取一份，`sourceKey` 用 `source_<id>`
   （与上游 Suwayomi 的 `preferenceKey()` 一致，`eu/kanade/tachiyomi/source/ConfigurableSource.kt:21`）；
3. 写入 105 号段。空列表的源不进备份（对齐 Mihon 的 `.filter { it.prefs.isNotEmpty() }`）。

被否的备选是「把 `PreferenceScreen` JSON 原样塞进 900x 号段」：只动本仓，但与 Mihon 不互通，
且形态绑死沙盒的序列化细节。

### 3.4 包含敏感设置（`privateSettings`）

Mihon 用 key 前缀 `__PRIVATE_` 过滤。本仓可映射的敏感项有两处，
其中一处**现在无条件导出**：

| 位置 | 现状 |
| --- | --- |
| `tracker_credential` 表 → proto 9002（token / password / pkce_verifier） | 跟随 `includeTracking`，**无条件**带出（`backup.rs:945`） |
| `BackupServerSettings.auth_username` / `auth_password`（9001 tag 5/6） | 9001 恒为 `None`，尚未导出；一旦实现「应用设置」就会带上 |

**已选：默认关闭，且纳入 tracker 凭据与认证信息。** 即 9002 的 tracker 凭据不再跟随
`includeTracking` 无条件带出，改为受 `privateSettings` 控制（关闭时不导出）；9001 的
`auth_username` / `auth_password` 同理。**这会改变当前的默认备份内容**（现在凭据一定会进备份）。

### 3.5 应用设置（`includeAppSettings`）

**已选：新增 `includeAppSettings`，旧的两个字段废弃。** 实现约定：

| 事项 | 约定 |
| --- | --- |
| GraphQL | `PartialBackupFlagsInput` 新增 `includeAppSettings: Option<bool>`，默认 `true` |
| 旧字段 | `includeClientData` / `includeServerSettings` 保留但标 `@deprecated`（不删：上游客户端与旧 WebUI 仍在传），解析时任一为 `true` 即 `appSettings = true` |
| proto 承载 | 9001 `serverSettings`（服务端设置）+ 9000 `meta`（客户端 meta）；**不填 Mihon 的 104** |
| 边界 | 进 9001 的项排除 `auth_username` / `auth_password`（归 `privateSettings`）与 `ip` / `port`（恢复到另一台机器会直接改网络监听地址） |

不填 104 的理由：104 是客户端 SharedPreferences 的键值形态，本仓的服务端设置是结构化的 9001，
要写进 104 得另定一套键名映射、且与 Mihon 的真实键也对不上；上游 Suwayomi 同样没填
（它的 proto 顶层就是 1/2/101/9000/9001，`impl/backup/proto/models/Backup.kt`）。
若要求与 Mihon 的 104 双向互通，需单独定键名映射，不在本次范围。

后端兑现这个开关的内容：9001 现在恒为 `None`（`backup.rs:966`）、9000 恒为空 map（`:965`）。
「服务端设置」落在 `server_settings` 表，「客户端数据」对应上游的 `manga_meta` / `chapter_meta` /
`category_meta` / `source_meta` / `global_meta` 五张表 —— 本仓都有表，只是没填进备份。
本轮一并实现这两处填充（否则开关名实不符）；若要缩范围，可先只做字段与 UI，填充另开一轮。

---

## 4. 顺带发现的既有问题（不在本次需求内，但会被本次触碰）

1. **`backup.rs:551-556` 的注释断言了错误事实**，并据此把 `in_library` 硬编码为 `TRUE`。
   影响：恢复 Mihon 备份（默认含非库已读作品）会把它们全部入库。详见 3.1。
2. `includeHistory` 无数据源：本仓没有 history 表（`suwayomi-db` 的 `BUSINESS_TABLES` 无此项）。
   `chapter.last_read_at` 可作为 `BackupHistory.read_at` 的近似，`read_duration` 只能给 0。
   本次需求未涉及「History」开关，可先维持现状（UI 上仍是一个空操作的框）。
3. `includeClientData` / `includeServerSettings` 空操作 —— 由 3.5 一并处理。

---

## 5. 实施分层（决策后照此推进）

| 层 | 改动 |
| --- | --- |
| L1 proto | `Backup` 加 **105/106**（104 不填，理由见 3.5）；新增 `BackupSourcePreferences` / `BackupPreference` / `BackupExtensionStore` 三个消息，tag 逐字对齐 Mihon；`PreferenceValue` 用 oneof |
| L2 flags | 新增 `readEntries` / `extensionStores` / `sourceSettings` / `privateSettings` / `appSettings`，去掉 `clientData` / `serverSettings` → 最终 10 项，与 Mihon `BackupOptions` 一一对应；`to_array` / `from_array` 长度同步；`BACKUP_FLAG_QUERY_KEYS` 同步；扩现有单测 |
| L3 导出 | `build_backup`：非库已读查询 → `backupManga`（3.1）、`extension_store` 表 → 106、沙盒偏好 → 105、9000 `meta`、9001 服务端设置（排除 auth/ip/port）、`privateSettings` 过滤 |
| L4 恢复 | `restore_backup`：`in_library` 跟随 `favorite`（**待定，见下方未定项**）；105/106 的应用与忽略策略 |
| L5 GraphQL | `PartialBackupFlagsInput` 加 `includeAppSettings`、旧两字段标 `@deprecated`；`backup_flags()` 映射 |
| L6 WebUI | `Backup.types.ts` / `Backup.constants.ts`（文案、分组）/ `Backup.utils.ts`（两个映射）/ `BackupFlagInclusionDialog`（灰化依赖）/ 「创建」按钮的 `canCreate` 禁用；generated 4 个文件重生成；`zh-Hant` 文案补齐 |
| L7 沙盒 | `ext-runtime` 加 `GET /source/{id}/preferences/raw`，返回扁平 key/value（3.3） |

**已定（2026-09-29）**：「所有已读作品」的恢复侧语义（3.1）取 Mihon 语义 —— `in_library`
跟随 `BackupManga.favorite`（`backup.rs` 的 `let in_library = m.favorite;`），
否则 `readEntries` 无法往返自洽。

**已一并处理**：自动备份的服务端设置。`autoBackupInclude*` 7 项与 `Backup.utils.ts` 的两个映射
是 1:1 的，按 L2 的最终集合，这组设置也要跟着改名/增删
（`autoBackupIncludeClientData` + `autoBackupIncludeServerSettings` → `autoBackupIncludeAppSettings`，
再加 4 项）。它是跨仓库的 schema 变更（设置名进 GraphQL 与 WebUI 设置页），
不随本轮做的话，手动/自动两侧的开关集合会长期不一致。

## 6. 验证

- 单测：`BackupFlags` query 往返；`build_backup` 各 flag 的内容断言（参照现有 `backup_flags` 测试）。
- 往返幂等：导出 → 导入 → 再导出，字节一致（现有测试已覆盖部分路径，需扩展到新增项）。
- **Mihon 侧兼容**：解出 105/106，逐字段比对 tag 号与名称；再拿一份真机 Mihon 备份验证
  105/106 能被本仓解出。
- **恢复行为**：拿一份含 `favorite = false` 条目的 Mihon 备份，确认恢复后这些作品不会出现在书库
  （当前代码会把它们全部入库）。
- 端到端：LATEST(4567) + CDP 打开 `/settings/backup`，跑 `backup_flags_probe.py` 确认各组合产出不同指纹。

---

## 7. 落地情况（2026-09-30）

§5 的 L1–L7 全部落地，`readEntries` 的往返语义按 §5 的「已定」项实现：

| 层 | 结果 |
| --- | --- |
| L1 proto | `Backup` 的 105 / 106 段与三个新消息已加，tag 对齐 Mihon |
| L2 flags | `BackupFlags` = 10 项（`includePrivateSettings` 默认 false，其余 true）；`to_array` / `from_array` / `BACKUP_FLAG_QUERY_KEYS` 同步 |
| L3 导出 | 10 项全部生效：非库已读（`readEntries`）、`extension_store` → 106、沙盒偏好 → 105、9000 `meta`、9001 服务端设置（排除 auth / ip / port）、`privateSettings` 过滤（9002 凭据 + 9001 认证） |
| L4 恢复 | `in_library` 跟随 `favorite`；105 经沙盒写回、106 回落 `extension_store` 表 |
| L5 GraphQL | `PartialBackupFlagsInput` 10 项 + 旧两字段 `@deprecated`（解析时任一为 true → `appSettings`，且不回写旧键）；`SettingsType` 的 `autoBackupInclude*` 同步 10 项 |
| L6 WebUI | 10 个复选框 + 灰化依赖 + 「创建」按钮的内容判定（`hasBackupContent`）；`zh-Hans` / `zh-Hant` 译文补齐 |
| L7 沙盒 | `ext-runtime` 的 `GET /source/{id}/preferences/raw` |

验证（2026-09-30 复跑）：

- `cargo test -p suwayomi-core -p suwayomi-graphql`：12 项 backup 单测含 §6 的往返幂等与
  「`favorite = false` 的作品恢复后不进书库」，全绿。
- `.workbuddy/verify/backup_picker_check.mjs` **11/11**：勾选对话框 10 个开关、全不选时 Ok 禁用、
  勾选真的改变落盘字节、导出 URL 带 10 个开关。
- `.workbuddy/verify/backup_restore_cancel_check.mjs` **7/7**：取消恢复对话框不产生 unhandled
  rejection、不发 `restoreBackup`、清空 input 后重选同一文件仍弹框。

遗留：

- 「扩展商店」页按商店统计 NSFW / 已安装数量走的是 `extensions(condition: { storeIndexUrl })`
  与 `filter: { contentWarning: … }`。服务端本轮才补上这两个入参与过滤，GraphQL 层已实测
  （全量 6 / `>= MIXED` 5 / `= SAFE` 1），但本机实例没有 `extension_store` 行，页面上的角标
  无法在本地跑出来。
- 勾选对话框里从属开关被灰化后仍保持勾选值（依赖项关掉时不会一并归零），取消不干净时导出的
  flags 会出现 `includeManga=false` 与 `includeChapters=true` 并存。要不要改成联动归零属 UI 语义选择。
