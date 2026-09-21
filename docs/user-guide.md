# Suwayomi (next) 用户指南

Suwayomi (next) 是 Suwayomi（Kotlin/JVM 版）的 Rust 重写：保持既有数据格式、
GraphQL / REST / OPDS 接口与 Mihon 扩展体系兼容，默认**零外部依赖**启动
（内建 SQLite 数据库，可选切换到外部 PostgreSQL）。

## 快速开始

```bash
# 直接运行（默认端口 8090；工作数据存 ./data/，SQLite 库存 ./db/suwayomi.db）
cargo run --release -p suwayomi-server
# 或使用已构建二进制
./target/release/suwayomi-server
```

- WebUI：`http://localhost:8090`（托管目录，见下）
- GraphQL：`http://localhost:8090/api/graphql`
- REST：`http://localhost:8090/api/v1`
- OPDS：`http://localhost:8090/api/opds/v1.2`（KOReader 等阅读器）

## 配置（环境变量）

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `SUWAYOMI_PORT` | `8090` | HTTP 端口 |
| `SUWAYOMI_IP` | `0.0.0.0` | 监听地址 |
| `SUWAYOMI_DATA_DIR` | exe 上级 `data/` | 数据目录（下载/本地图源/自动备份之下；也可在 WebUI 的「数据与存储 → 存储位置」里改） |
| `SUWAYOMI_DB_DIR` | exe 上级 `db/` | 数据库目录（**与数据目录分开**，见下） |
| `SUWAYOMI_SQLITE_PATH` | `<数据库目录>/suwayomi.db` | SQLite 数据库文件路径（显式指定时不做旧库迁移） |
| `SUWAYOMI_DB_BACKEND` | `sqlite` | 后端：`sqlite` / `postgres` |
| `SUWAYOMI_DATABASE_URL` | （空） | PostgreSQL 连接串（设置后自动改用外部 PostgreSQL，如 `postgres://user:pass@host:5432/db`） |
| `SUWAYOMI_AUTH_MODE` | `DISABLED` | 认证模式：`DISABLED` / `BASIC_AUTH` / `SIMPLE_LOGIN` / `UI_LOGIN` |
| `SUWAYOMI_AUTH_USERNAME` / `SUWAYOMI_AUTH_PASSWORD` | — | 认证凭据（用户名与密码都不能为空，否则服务端拒绝启动） |
| `SUWAYOMI_JWT_AUDIENCE` | `suwayomi-server-api` | JWT 的 `aud` 声明 |
| `SUWAYOMI_JWT_TOKEN_EXPIRY` | `5m` | 访问令牌有效期（也接受 `PT5M` 这类 ISO-8601 写法） |
| `SUWAYOMI_JWT_REFRESH_EXPIRY` | `60d` | 刷新令牌有效期 |
| `SUWAYOMI_SESSION_SECRET` | — | 会话 cookie 与 JWT 的签名密钥；未设置时首次启动生成 `<数据库目录>/session.key` |
| `SUWAYOMI_AUTH_COOKIE_SECURE` | — | 设为 `1` 时给会话 cookie 加 `Secure`（仅 HTTPS 反代后开启） |
| `SUWAYOMI_SANDBOX_JAR` | — | JVM 扩展沙盒 jar 路径（未设置则扩展源不可用） |
| `SUWAYOMI_SANDBOX_PORT` | `8091` | 沙盒 HTTP 端口 |
| `SUWAYOMI_EXTENSIONS_DIR` | `./extensions` | 扩展 APK 目录（只放 APK） |
| `SUWAYOMI_JAR_DIR` | `<extensions>/../bin/extensions` | dex2jar 转换产物 jar 目录 |
| `SUWAYOMI_SETTINGS_DIR` | `<扩展目录>/../settings` | 设置目录（沙盒的源偏好与追踪器凭据都在这里） |
| `SUWAYOMI_TRACKERS_CONFIG` | `<设置目录>/trackers.json` | 追踪器 OAuth 应用凭据文件（见下） |

## 认证

默认 `DISABLED`（不认证）。开启后，**所有数据接口都要凭据**：`/api/v1/**`、
`/api/graphql`（含 WebSocket 订阅）、`/api/opds/v1.2/**`、`/api/v1/local/**` 与
`/local/**`。WebUI 的静态产物（`/assets/*`、`/favicon.svg`、`/sw.js` 等）匿名可读——
否则登录页本身都加载不出来——但 `index.html` 不在豁免之内。

| 模式 | 凭据通道 | 未认证时 |
| --- | --- | --- |
| `DISABLED` | 不需要 | 一切照常（启动时日志会打一条 warning） |
| `BASIC_AUTH` | 每个请求的 `Authorization: Basic` | 页面与接口都回 `401` + `WWW-Authenticate` |
| `SIMPLE_LOGIN` | 登录一次换取会话 cookie（30 分钟） | 页面 `303` 到 `/login.html`，接口 `401` |
| `UI_LOGIN` | 登录一次换取访问 / 刷新令牌（JWT） | 接口 `401`，页面照常返回外壳 → WebUI 自己弹登录页 |

**登录页**：`GET /login.html` 是服务端自渲染的最小表单（内联样式、零外部资源），
`POST /login.html`（字段 `user` / `pass`）成功后种下签名会话 cookie 并 `303` 回
`?redirect=` 指定的站内路径。`GET /logout` 清 cookie。`UI_LOGIN` 的 WebUI 走的是
GraphQL 的 `login` / `refreshToken`，不需要这两个页面。

**别把服务暴露到公网还开着 `DISABLED`**；`BASIC_AUTH` / `SIMPLE_LOGIN` 只做认证不做
传输加密，公网部署请套 HTTPS 反代并设 `SUWAYOMI_AUTH_COOKIE_SECURE=1`。

**`?token=` 查询参数**只在 OPDS 与章节取页路径上接受
（`/api/opds/**`、`/api/v1/manga/{id}/chapter/{n}/page/{i}`）。查询参数会进访问日志与
浏览器历史，所以其余接口只认请求头或 cookie。

**CSRF**：靠 cookie 认证且方法不是 GET/HEAD/OPTIONS 的请求，会校验 `Origin` 同源与
`Sec-Fetch-Site`。用 Basic 或 Bearer 的请求不受影响（浏览器不会自动带上它们）。

**设置页**：WebUI 的「设置 → 高级 → 服务器设置 → 认证」可以改模式与凭据。改动先落在
草稿里，`Save` 才提交、`Discard` 丢弃。认证参数是**启动时读取**的——保存后需要重启服务端
才生效。环境变量优先于设置页里保存的值；两者都空则用默认值。清空凭据并保持认证开启会让
服务端拒绝启动（页面上会拦住这个组合）。

`DISABLED` 下不签发任何凭据：`login` 与 `POST /login.html` 一律失败。关掉认证的实例上
拿不到令牌，所以也就不存在「趁认证关着先换好令牌、等管理员打开认证后继续用」这条路。

### 从旧版迁移

旧版的 `SIMPLE_LOGIN` 只拦页面，`/api/**` 完全不设防。升级后这部分接口需要凭据：

- WebUI 用户无感（浏览器带着会话 cookie）。
- 直连 API 的脚本 / 第三方客户端要给 Basic 凭据，或先 `POST /login.html` 拿到
  `logged-in` cookie 再带着它请求。
- `/api/v1/local/**` 在本项目里也属于数据接口，同样需要凭据。

## 追踪器登录

五个追踪器（MyAnimeList / AniList / Kitsu / Shikimori / Bangumi）登录时用的是**应用级**
OAuth 凭据。首次启动会在设置目录下生成 `trackers.json`（默认值 = 上游 Suwayomi 注册的
应用；Bangumi 授权页上显示的应用所有者就是上游的注册者）：

```json
{
  "bangumi":   { "clientId": "bgm…", "clientSecret": "…", "redirectUri": "https://suwayomi.org/tracker-oauth" },
  "shikimori": { "clientId": "…",   "clientSecret": "…", "redirectUri": "https://suwayomi.org/tracker-oauth" },
  "kitsu":     { "clientId": "…",   "clientSecret": "…" },
  "anilist":   { "clientId": "16186" },
  "mal":       { "clientId": "…" }
}
```

- **在哪改**：设置 → 进度记录，每个追踪器右侧的齿轮（应用凭据）里填，保存即生效 ——
  落到这个文件、也立刻用在新登录与刷新上。等价的改法是直接编辑这个文件再重启服务端。
- 想用自己的应用：到站点开发者页注册（Bangumi 是 <https://bgm.tv/dev/app/create>），把
  `clientId` / `clientSecret` 填进去。`redirectUri` 可以继续沿用
  `https://suwayomi.org/tracker-oauth`——它是上游网站上的一个转发页，把授权码原样转回
  本机 WebUI，不要求归你所有；换成自己的回调地址时必须同时在站点应用里登记。
  **界面里留空 = 回到内置默认值**（不是「不配置」）。
- **缺键用内置默认值补齐**，所以只写要改的站点即可；文件已存在就**永不重写**（只有设置页
  保存会写），写坏了只会在日志里告警并退回默认值。文件里显式写成空串表示「不配置」，
  用到时报错 —— 界面不会写出这种值。
- 换了 `clientId` 等于换了应用，**旧 token 刷不了**，需要重新登录一次。
- 用户自己的 token 不在这个文件里（在数据库的 `tracker_credential` 里）。

## 从 Kotlin 版迁移

Kotlin 版使用 H2 数据库文件（JVM 专有格式，Rust 无法直读），迁移走 Mihon `.proto`
备份导入（`POST /api/v1/backup/import`）。完整操作指南见 **`docs/migration/MIGRATE.md`**。

## 备份

- 导出（流式 gzip protobuf）：`GET /api/v1/backup/export`
- 导出文件：`GET /api/v1/backup/export/file`（`org.suwayomi.next_<ts>.tachibk`）
- 导入：`POST /api/v1/backup/import`（body 为 gzip 备份）

## OPDS / KOReader

根目录：`http://localhost:8090/api/opds/v1.2`
（支持：库浏览、来源探索、历史、库更新、系列章节、章节元数据；`?lang=` 切换语言）

## Docker

官方镜像在 GHCR（`linux/amd64` 与 `linux/arm64` 多架构），标签与发布版本一致：alpha 为 `r<提交数>`、release/beta 为 `3.y.z`。**没有 `latest` 标签**，请显式指定：

```bash
docker run -p 8090:8090 -v suwayomi-data:/data ghcr.io/576576/suwayomi-next:r3226
```

本地构建（`WEBUI_URL` 指向 Suwayomi-WebUI 的 release zip，不传则镜像不含 WebUI）：

```bash
docker build -t suwayomi-next --build-arg WEBUI_URL=<zip 地址> .
docker run -p 8090:8090 -v suwayomi-data:/data suwayomi-next
```

数据与 SQLite 库都持久化在 `/data`（工作数据在 `/data`，库在 `/data/db/suwayomi.db`）。要连外部 PostgreSQL：

```bash
docker run -p 8090:8090   -e SUWAYOMI_DB_BACKEND=postgres   -e SUWAYOMI_DATABASE_URL=postgres://user:pass@host:5432/db suwayomi-next
```

## 已知限制

- 默认 SQLite 为单写者模型（WAL + busy_timeout），适合单实例部署；需要
  多实例并发写请切到外部 PostgreSQL。
- 真实扩展源（Mihon APK→JAR）依赖 JVM 沙盒（`SUWAYOMI_SANDBOX_JAR`）；
  未配置时来源相关端点返回"source unavailable"。
- WebUI 只在内存里保存访问令牌，刷新页面后靠持久化的刷新令牌重新换取。换令牌之前
  发出的请求会先拿到一次 `401`，客户端据此触发展开——服务端访问日志里出现零星
  `401 /api/graphql` 属正常。
