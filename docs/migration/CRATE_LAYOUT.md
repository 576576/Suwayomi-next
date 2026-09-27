# crate 分层梳理与 `suwayomi-api` 抽取（2026-09-27）

三个问题的答复：① `suwayomi-graphql` 与 `suwayomi-rest` 都是对外接口，能不能整合；
② 其它 crate 各自干什么；③ 每个 crate 的版本都停在 `0.1.0`，能不能给语义化版本。

结论：**协议层不合并，抽公共层**；顺带修掉两个真 bug（版本号泄漏、双 `RuntimeConfig`）。

## 一、现状依赖图（严格单链，36,398 行 `src/**/*.rs`）

```
suwayomi-db-macros → suwayomi-db → suwayomi-core → suwayomi-domain
                                                        │
                                                        ▼
                                                  suwayomi-api
                                                   │   │    │
              ┌────────────────────────────────────┘   │    └──────────────────────┐
              ▼                                        ▼                           ▼
        suwayomi-rest                         suwayomi-graphql               suwayomi-opds
              └────────────────────────────────────────┼───────────────────────────┘
                                                       ▼
                                         suwayomi-server → suwayomi-android
```

| crate | 行数 | 职责 | 为什么单独存在 |
|---|---:|---|---|
| `suwayomi-db-macros` | 85 | `#[derive(FromRow)]` 过程宏 | proc-macro 必须独立 crate（编译期与运行期分离） |
| `suwayomi-db` | 2,525 | 双后端数据库层（默认 SQLite / 可选 PostgreSQL）+ 迁移 | `sqlx` 风格的查询 API 与业务无关；两种后端要在同一 API 下切换 |
| `suwayomi-core` | 3,341 | 领域模型、数据表行类型、配置、认证原语、`build.rs` 版本注入 | 被 domain 与 api 同时依赖，不能挂在任一业务层下 |
| `suwayomi-domain` | 13,771 | 业务逻辑，**唯一对外发 HTTP 的一层** | 协议层只做翻译，不发请求 |
| `suwayomi-api` | 691 | **本次新抽**：`AppState`（21 字段的共享句柄）+ 全站认证中间件 | 见第三节 |
| `suwayomi-rest` | 1,791 | REST `/api/v1/**` | 契约形状独有（见第二节） |
| `suwayomi-graphql` | 10,629 | GraphQL schema + 订阅 + WebSocket | 同上 |
| `suwayomi-opds` | 2,348 | OPDS 1.2 / 2.0 + KOReader 的 `/opds/**` | 同上 |
| `suwayomi-server` | 1,005 | 装配层：建全部有状态服务、nest 三个协议路由、静态托管、JVM 沙盒子进程 | 唯一知道"怎么把东西拼起来"的地方 |
| `suwayomi-android` | 212 | JNI `cdylib`，Android 上整份 server 随 APK 分发 | 产物类型不同（动态库 vs 可执行文件） |

## 二、为什么 `rest` 与 `graphql` **不**合并

三条实测理由：

1. **编译期零依赖**：两者互不引用，不存在"同一份代码写了两遍"。
2. **运行时早已整合**：`suwayomi-server::build_router` 把三家 `nest` 进同一个 axum
   `Router`，监听同一个端口、同一个 `SocketAddr`。对外只有一个文件源（OPDS 与下载页图片
   走同一个进程），不存在两个服务。
3. **契约形状不同形**：REST 的错误是 `{"message": …}` + 语义化状态码；GraphQL 是
   HTTP 200 + `errors` 数组 + `data` 部分成功。合并只会把两套错误映射塞进一个 crate，
   谁也没少写。

真正重复的是**状态与认证**：两个 `AppState` 各 21 字段、17 个同名，
`suwayomi-opds` 为了拿一个类型而反向依赖 `suwayomi-rest`。

## 三、做法：抽 `suwayomi-api`（协议无关的 HTTP 公共层）

只放两样东西：

- `AppState` —— 装配层建一次、三个协议层当 axum state 用；
- `auth` —— **全站**中间件（含 WebUI 静态与 `/api/v1/shutdown`），不是 REST 专属。

**不放**错误映射：那是各协议自己的契约形状。

| 动作 | 文件 | 说明 |
|---|---|---|
| 搬迁 | `rest/src/{state,auth}.rs` → `api/src/` | `git mv`。`auth.rs` 551 行**一字未改**（git 记 100% rename）——它写的是 `crate::state::AppState`，两个文件同 crate 平级搬迁后路径照样解析；`state.rs` 只改了模块级文档注释（89%） |
| 改依赖 | `opds/Cargo.toml` | 依赖 `suwayomi-rest` → `suwayomi-api`：**消除唯一的层间反向依赖** |
| 改导入 | `rest/src/routes*.rs`（13 文件）、`opds/src/router.rs`、`server/src/lib.rs`（7 处 auth + 1 处 state） | `crate::state::AppState` → `suwayomi_api::AppState` |
| 去重 | `graphql/src/state.rs` | `GraphQLState` 不再抄 17 个字段，改为持有 `AppState` 并 `impl Deref<Target = AppState>` —— 207 处 `state.<field>` 一个字不用改，而"共享"这件事表达在类型上 |
| 装配 | `server/src/lib.rs` | 顺序反转为**先 `AppState` 后 `GraphQLState`**（后者由前者派生） |

## 四、顺带修掉的三个缺陷

### 1. `/api/v1/settings/about` 报 `version: "0.1.0"`

`routes/global.rs` 的 `about` 原来返回 `env!("CARGO_PKG_VERSION")` —— 那是**本 crate 的
`Cargo.toml` 版本**，在 workspace 里恒为 `0.1.0`，而 GraphQL 的 `aboutServer.version` 读的是
`suwayomi_core::version::VERSION`（`core/build.rs` 由 commit count 推导）。
同一个 server 两个接口报出两个版本号。改成同一个常量。

### 2. 双 `RuntimeConfig` —— 改设置对 REST 不生效

`RuntimeConfig` 是 `Arc<RwLock<ServerConfig>>` 句柄。`GraphQLState::new` 与 `AppState::new`
各自 `new` 了一份，而 `reload_runtime_config()`（启动一次 + `setSettings` 一次）只写
GraphQL 那份 → REST / OPDS 读到的永远是 env 基线：设置页把 `opdsCbzMimetype` 改成别的，
`rest/src/routes/chapter.rs` 取页仍按旧值出图。

这和 `333a73d` 修掉的 `DownloadManager` 双队列是同一类缺陷：**带内部状态的服务只能在
装配层建一次**。修法是 `GraphQLState` 持有 `AppState`（第三节），`config` 只剩一份。

### 3. tracker 的 User-Agent 里也是 `0.1.0`

`domain/src/tracker.rs` 的 `USER_AGENT` 常量写成
`concat!("Suwayomi-next/", env!("CARGO_PKG_VERSION"))` —— 第三方站点（MAL 缺 UA 直接拒）
收到的是一个无意义的版本号。改成 `user_agent()` 函数（`OnceLock` 一次性初始化，
返回 `&'static str`，调用点形状不变），7 个 tracker 文件共 19 处跟着改。

> 为什么不能继续用 `concat!` 常量：`concat!` 只吃字面量，而 `cargo:rustc-env` 注入的变量
> **只对有该 build script 的包可见** —— domain 没有 `build.rs`，拿不到
> `SUWAYOMI_VERSION_NAME`。

## 五、版本号：`0.1.0` 本身不是 bug，暂时不动

现状是所有 crate `version.workspace = true` 继承 `[workspace.package] version = "0.1.0"`，
且所有 path 依赖**不带 version 需求**。后果：

- Cargo 只把它记进 `Cargo.lock`，**不参与解析**；
- 没人 `cargo publish`（产物是 exe/msi/APK，不是 crate），所以 semver 的"兼容性承诺"
  没有任何接收方；
- 产品版本另有来源：`core/build.rs` 由 **commit count** 推导（alpha `r{code}`，
  release/beta 是 CI 注入的 `3.y.z`）。

所以把 10 个 crate 各写一个 `1.0.0` / `0.2.0` 是纯装饰，还会造出一批"看起来有语义、
实际没有"的数字。**真正要修的是消费方**（第四节 1、3），已完成。

如果之后确实要语义化（例如开始对外发布 crate，或想让 `cargo metadata` 能表达兼容性），
做法是把 `[workspace.package] version` 与各 crate 脱钩，按"该 crate 的公开 API 是否
破坏"逐条定 `0.x.y`；这是独立一步，不影响当前功能。

## 六、验收

新增契约断言：`graphql/src/schema.rs` 的 `tests::schema_matches_baseline`。
`include_str!` 把 `docs/graphql/schema-baseline.graphql` 在编译期嵌进测试二进制，
断言本 fork 产出的 SDL 与基线的差集**恰好**是已知的 17 个未实现 + 8 个自有。
顺带把 `schema_type_count()` 的口径对齐基线（原先漏数 `union`，日志少报 2 个）。
细节与差集清单见 `docs/graphql/README.md` 的「Rust 侧现状」。

三道门（本地，`DATABASE_URL` 指向嵌入式 PostgreSQL 16.15）：

| 门 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过（10 个 crate，零告警） |
| `cargo test --workspace --no-fail-fast` | **182 通过 / 0 失败**（原 180 + 新增 2 条契约断言） |

20 个依赖 PostgreSQL 的用例**真跑**（不是静默 skip）：`core/tests/db_pg.rs` 3 例
（0.54 s）、`domain/tests/services.rs` 10 例（2.59 s）、`domain` 内联
`extension_store` / `sync_yomi`，均连库执行。
