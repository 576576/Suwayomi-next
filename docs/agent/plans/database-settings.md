# 数据库设置：把设置页的四个字段接到真实后端

目标：让设置里那组数据库字段（`databaseType` / `databaseUrl` / `databaseUsername` /
`databasePassword`，外加 `useHikariConnectionPool`）**真的决定库开在哪**，而不是像现在
这样存下来、显示回去、然后被忽略。

核对基准：

| 参考实现 | commit | 关键文件 |
| --- | --- | --- |
| Suwayomi-Server | `d37230ee` | `server/server-config/src/main/kotlin/suwayomi/tachidesk/server/ServerConfig.kt`、`AndroidCompat/Config/.../ConfigManager.kt`、`server/.../ServerSetup.kt` |

术语约定：下文**参考实现** = `Suwayomi/Suwayomi-Server`（Kotlin 版）。本项目与它在
数据模型与接口上兼容，但**后端实现不同**：它跑 JVM（H2 / PostgreSQL + Hikari），本仓
跑原生 Rust（SQLite / PostgreSQL）。

---

## 0. 现状（2026-09-30）

真实后端只由环境变量决定：

```
crates/suwayomi-db/src/config.rs   DbSettings::from_env()
  SUWAYOMI_DB_BACKEND=postgres  → BackendKind::Postgres
  其余（含未设置、写错）        → BackendKind::Sqlite（<appdata>/db/suwayomi.db）
```

而设置页那四个字段走的是另一条链，**终点是显示**：

| 环节 | 位置 |
| --- | --- |
| 写入 | `setSettings` → `mutation_b4.rs:2190` 起，落进 `global_meta['settings']` 的 JSON blob |
| 读进配置 | `ServerConfig::apply_settings_blob`（`suwayomi-core/src/config.rs:272-290`） |
| 回读显示 | `state.rs:95-114`：`ServerConfig` 上盖一层 blob → `Query.settings` |

`run()` 里开库用的是 `db_settings.unwrap_or_else(|| DbSettings::from_env(...))`
（`suwayomi-server/src/lib.rs`），**不读 blob**。于是设置页显示的是"你存了什么"，
不是"正在用什么"。

另外两条事实（决定了后面能怎么接）：

- **嵌入式 Oliphaunt 已删除**（`suwayomi-server/Cargo.toml:40` 的注释），所以
  `DatabaseType::H2` 在本项目里没有对应实现 —— H2 是 JVM 专有格式，本仓读不了。
- **没有 Hikari**：PostgreSQL 侧是 `deadpool`，池大小写死 `max_size(24)`
  （`suwayomi-db/src/backend/postgres.rs:30`）。`useHikariConnectionPool` 只是一个
  参考实现 schema 里的字段（备份协议 `backup.rs:420-431` 也带着它）。

---

## 1. 为什么不能把这一项存进库

**库就是被这一项选出来的。** 把选择结果写进 `global_meta` 后，改完设置重启时读到的
还是旧库里的旧值 —— 换了 `databaseType` 就永远读不到新值，这正是"设置不生效"的根因，
不是"忘了读"。同理，它也不能靠"重启时读文件"以外的方式持久化。

参考实现正是这么做的：整个 `ServerConfig` 存在**库之外**的 HOCON 文件
`<ApplicationRootDir>/server.conf`（`ConfigManager.userConfigFile`），
`-Dsuwayomi.tachidesk.config.server.<字段>` 可覆盖；`ServerSetup.kt:347` 还订阅了
`DatabaseSettings` 五个字段的组合流，变化时直接 `databaseUp()` 重开库。
我们只做到了"env 决定 + 库内展示"，两边的口径是断的。

---

## 2. 选型：引导文件 `<appdata>/settings/database.json`

放在这里而不是库里、也不是别的目录，理由有三条：

- 它在库**之前**就可读：库在 `<appdata>/db/`，`settings/` 是它的兄弟目录；
- 这个目录本来就是"库之外的进程状态"：`trackers.json`、`session.key` 同处，
  容器里随 appdata 卷一起走，不需要新的可写根；
- 与 `tray.json` 同构：都是"某个进程写在 appdata 下的设置文件"。

文件内容与设置项一一对应（不做 DSN 拼接的魔法）：

```json
{
  "backend": "postgres",
  "url": "postgres://host:5432/suwayomi",
  "username": "suwayomi",
  "password": "…"
}
```

**优先级：env → 文件 → 默认**（`SUWAYOMI_DB_BACKEND` / `SUWAYOMI_DB_URL` 仍是最高档）。

- env 优先是刻意的：容器与单机部署没有设置页，env 必须能压过文件；这与
  `SUWAYOMI_APPDATA_DIR` 的语义一致（env 是部署级，文件是应用级）。
- **两者不一致时以 env 为准并 warn**，设置页上标明"被环境变量覆盖"。否则用户点完
  保存、重启、发现没变，就又回到今天这种"设置不生效"的体验。

写入侧：`setSettings` 把这四个字段同时写文件（仍是原来那份 blob 也照写，`Query.settings`
的回读要它），并返回/提示"重启后生效"（与 `dataDir` 同构）。

⚠️ **换后端不搬运数据**：SQLite ↔ PostgreSQL 之间没有迁移，换过去就是一个按迁移脚本
新建的空库。设置页必须在切换处明写这一点，否则用户以为"切过去就能看到书架"。

### 不采用的方案

| 方案 | 为什么不 |
| --- | --- |
| 存进 `global_meta`（现状） | 选的库是它自己，重启即失效（见第 1 节） |
| 只保留 env，把四个字段从 GraphQL 摘掉 | 与参考实现的 schema 差异扩大，且 WebUI 的调试信息页已经在展示它们 |
| 指向一个固定的 `suwayomi.conf`（照抄 HOCON 形态） | 本仓没有 HOCON 依赖；`settings/` 下已有一批 JSON 形态的状态文件，再加一种格式没有收益 |

---

## 3. 逐字段接线

| 字段 | 现状 | 接线后 |
| --- | --- | --- |
| `databaseType` | 只显示，`config_from_env()` 里写死 `Postgresql` | 枚举加 `SQLITE`；`H2` 保留但**选它即报错**（H2 文件读不了），默认 `SQLITE` |
| `databaseUrl` | 只显示 | `backend=postgres` 时作为连接串；落盘前校验非空且能解析成 PG 串，失败不落盘并回错 |
| `databaseUsername` / `databasePassword` | 只显示 | `DbSettings` 加这两个字段（**不并进 DSN**：`tokio-postgres` 的报错会带整条 URL，凭据会进日志），`describe()` 打码 |
| `useHikariConnectionPool` | 只显示，无实现 | **不接线**：接收但当成 no-op（baseline 与备份协议里有这个键）；将来要池旋钮就另立名字（如 `dbPoolSize`），别把一个 JVM 库的名字当开关 |

`DatabaseType` 加 `SQLITE` 的连带影响：

- `suwayomi-core/src/config.rs` 的 `DatabaseType`、`suwayomi-graphql/src/settings.rs`
  的 `GraphqlDatabaseType`、以及 `settings.rs` / `config.rs` 里两处字符串映射；
- **schema 基线不用改**：`schema_matches_baseline` 只比顶层类型定义名；
- **WebUI 侧必须重跑 codegen**（枚举多一个值），并且 gen 出的
  `ServerSettings.tsx` 注释里那句 "`SUWAYOMI_DB_BACKEND` / `SUWAYOMI_DATABASE_URL`"
  已经过期（后者早已改名成 `SUWAYOMI_DB_URL`），顺手改掉。

---

## 4. 前端（WebUI 仓）

`ServerSettings.tsx` 的「服务端设置」当初就没恢复「数据库」分区，注释里写明原因
（"由 env 决定，H2 与 Hikari 无对应实现"）。接线后重开这块，只放四个字段 +
两条只读信息：

- 三个可编辑项：类型（下拉）/ URL / 用户名 / 口令；
- 只读「当前生效」：`databaseType` 与 `databaseUrl` 回读的必须是**实际生效**的值
  （env 或引导文件），不是刚存的草稿；
- 「改完需重启」提示 + 「换后端不会搬运数据」警示。

---

## 5. 实施分层（决策后照此推进）

| 层 | 内容 | 落点 |
| --- | --- | --- |
| L1 | 引导文件读写 + 优先级（env → 文件 → 默认）+ 坏文件/坏 JSON 回退默认并 warn | `suwayomi-db/src/config.rs`、`suwayomi-server/src/lib.rs` |
| L2 | `setSettings` 写文件；`effective_settings()` 里的这四个字段改读"实际生效" | `suwayomi-graphql/src/mutation_b4.rs`、`state.rs` |
| L3 | 枚举加 `SQLITE`、`H2` 报错；`DbSettings` 加用户名/口令；`describe()` 打码 | `suwayomi-core/src/config.rs`、`suwayomi-db` |
| L4 | WebUI 数据库分区 + 重跑 codegen | `Suwayomi-WebUI` |
| L5 | user-guide 补「数据库设置」节；`ci_pack_check.py` 加回读一致性断言 | 文档 / 验证脚本 |

---

## 6. 验证

- **单测**：优先级解析（env 有没有都覆盖到）、坏文件不致命、`H2` 被拒；
- **端到端**：写引导文件 → 重启 → 启动日志里的 `database backend:` 换库；同时设 env
  与文件不一致时，日志出现"以 env 为准"的 warn，且库确实是 env 那个；
- **回读**：`Query.settings.databaseUrl` == 实际生效值（不是 blob 里的旧值）；
- `ci_pack_check.py`：文案（`只有它决定`）与回读口径。

---

## 7. 未决

- 口令是否明文落盘：倾向照参考实现（`server.conf` 也是明文）+ 日志里恒打码。
- 要不要支持"从 H2 搬库"：不支持 —— 那是 JVM 专有格式，唯一的路是 Mihon `.proto`
  备份导入（见 `docs/zh/user-guide.md`）。
