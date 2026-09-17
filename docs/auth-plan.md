# 认证与授权补齐计划

> 行号对应 2026-09-17 的 `main`（`aa3d2b5`）。本文只写结论与做法，不写推导过程。
>
> **状态（2026-09-18）**：§6 的四个阶段已实现并实机验收。§8 的裁决以最终实现为准：
> **A 改为部分支持**——`?token=` 只对 OPDS 与章节取页路径开放，其余接口不认；
> B 接成「env 优先、设置页保存的值兜底、重启生效」；C/D/E 按建议落地。
> 四种认证模式各自跑通了验收矩阵，设置页的草稿/保存与 WebUI 登录闭环见
> `docs/user-guide.md` 的「认证」节。

## 1. 问题

### 1.1 权限缺口（未授权即可触达）

| # | 问题 | 位置 | 未授权可达性 | 影响 |
| --- | --- | --- | --- | --- |
| S1 | **任意文件读取**：`webui_fallback` 把请求路径直接 `dir.join(rel)` | `crates/suwayomi-server/src/lib.rs:221-233` | `DISABLED` 裸奔；`SIMPLE_LOGIN` 下也只拦"非 `/api/`"页面路径 | `GET /../db/suwayomi.db` 返回 966656B（魔数 `SQLite format 3`）；`GET /C:/Windows/win.ini` 返回内容——**Windows 上 `Path::join` 遇盘符绝对路径会整体替换 base，可读任意盘符** |
| S2 | **同上，本地图源路由**：`local_file` 防了 `..`，没防盘符/UNC | `crates/suwayomi-server/src/lib.rs:158-166` | 同上 | `GET /local/C:/Windows/win.ini` 返回内容；`/api/v1/local/...` 变体同 |
| S3 | **SIMPLE_LOGIN 下所有数据接口裸奔**：`cookie_ok \|\| path.starts_with("/api/")` | `crates/suwayomi-rest/src/auth.rs:95` | 任何匿名请求 | GraphQL（含全部查询/mutation）、REST（含 `/api/v1/backup/export` 全量导出）、OPDS、`/api/v1/local/**`、WS 订阅全部无凭据可读 |
| S4 | **凭据可伪造**：只判断 cookie 里有没有 `logged-in=` 这个名字 | `crates/suwayomi-rest/src/auth.rs:89-94` | 任何匿名请求 | `Cookie: logged-in=x` 即视为已登录 |
| S5 | **豁免判定过宽**：`ends_with("login.html")` / `ends_with("manifest.json")` | `crates/suwayomi-rest/src/auth.rs:42-49` | — | 任意 `*/login.html` 路径被放行；白名单无法收敛 |
| S6 | **未识别模式静默降级**：`_ => Disabled` | `crates/suwayomi-rest/src/auth.rs:24-30` | — | `SUWAYOMI_AUTH_MODE=UI_LOGIN`（及任何拼写错误）→ **关闭认证**，无任何提示 |
| S7 | **静态资源被误拦**：`/assets/*`、`/favicon.svg`、`/sw.js` 等不在豁免名单 | `crates/suwayomi-rest/src/auth.rs:33-51` | — | issue #5 的白屏即由此与 S1 共同造成 |
| S8 | **路径参数未净化**：`pkg` 直接进 `cache_dir.join(format!("{pkg}.{ext}"))` | `crates/suwayomi-rest/src/routes/extension.rs:58-111` | 认证后 | 扩展名被固定为 png/jpg/webp，读取受限；`std::fs::write` 同源问题 |
| S9 | **`/api/v1/shutdown` 无条件豁免** | `crates/suwayomi-rest/src/auth.rs:39-41` | 任何匿名请求 | 依赖 handler 内的 loopback 判定生存，纵深不足 |
| S10 | **只有中间件一层授权**：全仓无端点级主体判定（上游有 `requireUser()` / `requireUserWithBasicFallback`） | 全仓 | — | 一条豁免写法就是一次裸奔（S3 即此）；`UI_LOGIN` 在该层缺席时无法实现 |

### 1.2 功能缺口

| # | 问题 | 位置 | 现状 |
| --- | --- | --- | --- |
| F1 | `SIMPLE_LOGIN` 没有登录页与登录端点 | `auth.rs:88-101` 只重定向到 `/login.html` | 该路径落到 SPA fallback（返回 `index.html`），叠加 S7 → 白屏 |
| F2 | 全仓**没有任何地方写 cookie** | 仅 `auth.rs:93` 读 | 即便补出页面也登不进去 |
| F3 | `login` / `refreshToken` mutation 是空桩 | `crates/suwayomi-graphql/src/mutation_b4.rs:1836-1855` | 恒返回 `access_token: ""` |
| F4 | 模式只认环境变量，设置页改的是死值 | `crates/suwayomi-server/src/lib.rs:42-44`（env）；启动仅恢复 `localSourcePath`/`dataDir`（`lib.rs:290`、`lib.rs:309`） | WebUI 能改、运行时不生效、重启不生效 |
| F5 | WebUI 无服务器登出入口，未处理 401 | `AuthManager.removeTokens` 只在切模式时调用 | 未认证时停在 `SplashScreen` |
| F6 | BASIC_AUTH 下 WS 认证未验证 | `lib.rs:139` 的 `.layer(auth)` 作用于 `/api/graphql` 的升级请求 | 浏览器 WebSocket 不能自定义头，需实测 |

## 2. 目标与原则

1. **默认拒绝**：除 §3.3 的公开资源，一切请求都要凭据。
2. **数据接口零匿名**：`/api/**`、`/local/**` 在任何非 `DISABLED` 模式下都要求凭据——SIMPLE_LOGIN 不再例外。
3. **豁免最小且精确**：只放行"未认证用户渲染登录流程所必需"的资源；判定用精确路径与前缀，不用 `ends_with`。
4. **凭据不可伪造**：会话值必须签名并带过期。
5. **失败要响**：未识别模式直接启动失败；缺凭据的 API 返回 401 而不是重定向。
6. **不破坏既有部署**：`DISABLED` 为默认，行为不变；Basic 通道保持 RFC 7617 兼容，第三方客户端（Mihon/Suwayomi 客户端、KOReader）不受影响。
7. **两层结构**：中间件只做页面级门禁（303 / 401），数据授权放在端点层（主体注入 + `require_user()`），与上游一致。豁免规则从"是否放行"退化为"是否重定向"，写错不会再变成裸奔。

## 3. 权限模型

### 3.1 主体

单用户模型（与上游一致，不引入多用户/角色）：

| 主体 | 判定 |
| --- | --- |
| `Anonymous` | 无凭据，或凭据无效/过期 |
| `Authenticated` | 凭据有效（用户名 == `auth_username`） |

中间件产出 `Principal`，注入请求扩展，供后续 handler（授权、审计日志）取用。

### 3.2 资源分类与策略

| 类别 | 匹配方式 | `Anonymous` | `Authenticated` |
| --- | --- | --- | --- |
| 登录流程 | `/login.html`、`/logout` 精确匹配 | 允许 | 允许 |
| 公开静态资源 | 见 §3.3 | 允许 | 允许 |
| SPA 入口与深链接 | 其余页面路径（`/`、`/library`、…） | **303 → `/login.html?redirect=<path>`** | 允许 |
| 数据接口 | `/api/**`、`/local/**` | **401**（JSON） | 允许 |
| WebSocket | `/api/graphql` 且带 `Upgrade` | **401**（拒绝握手） | 允许 |

**分流规则**：`path` 以 `/api/` 或 `/local/` 开头 → 401；其余（页面）→ 303。这样 SPA 内的 fetch/WS 拿到 401 可自行跳登录页，而直接在地址栏打开深链接的人会被送到登录页。

### 3.3 公开资源（豁免白名单）

判定改为**「可在 webui 目录内安全解析到真实文件，且不是 SPA 入口」**，而不是维护文件名清单——新增构建产物不会再次白屏：

```rust
/// 公开静态资源：safe_join 命中 webui 目录内的真实文件，且不是 SPA 入口。
/// index.html 必须走认证：否则未登录用户直接拿到应用外壳。
fn is_public_asset(webui_dir: &Path, path: &str, method: &str) -> bool {
    if method == "OPTIONS" { return true; }
    let Some(file) = safe_join(webui_dir, path) else { return false; };
    if file.file_name().is_some_and(|n| n == "index.html") { return false; }
    file.is_file()
}
```

显式允许的例外（不属于静态资源但必须匿名可达）：`/login.html`、`/logout`。

`version.txt`、`assets/**`、`favicon*`、`sw.js`、`registerSW.js`、`workbox-*.js`、`site.webmanifest`、`*.png` 由此自动覆盖。

### 3.4 静态服务加固（S1/S2 的根因）

`dir.join(rel)` 在 Windows 上不可用：盘符绝对路径会替换 base；`..` 不受限。统一走一个净化函数，**三处调用点全部替换**（`webui_fallback`、`local_file`、`extension.rs` 的 pkg 落盘）：

```rust
/// 把请求路径解析到 root 之下的真实文件，任何越界输入返回 None。
/// 解码与规范化之后才判定：axum 的 Path 提取器已做一次 percent-decoding，
/// 因此 `%2e%2e` 到这一步已经是 `..`。
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.replace('\\', "/");
    let mut out = root.to_path_buf();
    for seg in rel.trim_start_matches('/').split('/') {
        match seg {
            "" | "." => continue,
            ".." => return None,
            // 盘符（C:）与 NTFS ADS（a:b）都在此拦下
            s if s.contains(':') => return None,
            s => out.push(s),
        }
    }
    // 双保险：规范化后必须仍在 root 下
    let canon = out.canonicalize().ok()?;
    canon.starts_with(root.canonicalize().ok()?).then_some(canon)
}
```

`extension.rs` 的 `pkg` 另加字符集白名单（`[A-Za-z0-9._-]`，且不以 `.` 开头）。

### 3.5 豁免之外的必要动作

- `OPTIONS` 放行保留（CORS 预检）；其余方法一律过主体判定。
- `/api/v1/shutdown` **移出豁免**：改为要求 `Authenticated` **且** loopback（两层都留）。
- 未识别 `SUWAYOMI_AUTH_MODE` → **启动失败**并打印允许取值（`AuthMode::from_str` 返回 `Result`）。

## 4. 凭据通道

| 通道 | 格式 | 用在 | 生效模式 |
| --- | --- | --- | --- |
| Basic | `Authorization: Basic base64(user:pass)` | 浏览器原生弹窗、第三方客户端、KOReader | `BASIC_AUTH` |
| 会话 cookie | `Cookie: logged-in=<base64(payload)>.<HMAC>` | 浏览器（服务端登录页下发） | `SIMPLE_LOGIN` |
| Bearer | `Authorization: Bearer <JWT>` | WebUI | `UI_LOGIN`（§6 阶段 3） |

**会话 cookie 规格**：

- payload = `user:<username>:<exp_unix>`，`HMAC-SHA256(secret, payload)` 附加在后
- 属性：`HttpOnly; Path=/; SameSite=Lax`，配 `Secure` 可选（`SUWAYOMI_AUTH_COOKIE_SECURE=1`）
- 密钥：`SUWAYOMI_SESSION_SECRET` → 否则 `<db 目录>/session.key` 随机 32B 自动生成（首次启动，权限 0600）
- 校验：签名 + 过期 + 用户名比对，任一不过视为 `Anonymous`
- 改用户名/改密码/换密钥 → 旧 cookie 自动失效（用户名进 HMAC 输入）

**CSRF 纵深防御**：对 `POST/PUT/PATCH/DELETE` 且以 cookie 认证的请求，校验 `Origin`/`Sec-Fetch-Site` 为同源；不匹配返回 403。Basic/Bearer 不受影响。

## 5. 模式语义

四种取值不是四条并行方案，而是「谁拥有登录环节」的三条路线加一个关闭态。真正的差异只有两轴：**凭据载体**、**拦截位置**。

| 模式 | 凭据载体 | 登录环节归谁 | 拦截位置 |
| --- | --- | --- | --- |
| `DISABLED` | 无 | — | 不拦截 |
| `BASIC_AUTH` | `Authorization: Basic`（每请求） | 浏览器原生弹窗 | 中间件：整站 401 + `WWW-Authenticate` |
| `SIMPLE_LOGIN` | 会话 cookie（服务端 session） | 服务端渲染的 `/login.html` | 中间件：页面 303 → 登录页；数据接口另由端点层判 |
| `UI_LOGIN` | `Bearer <JWT>`（另接受 cookie `suwayomi-server-token` 与 `?token=`） | UI 自己（SPA 登录页） | **中间件无分支**；全靠端点层 |

上游文档对 `ui_login` 的定义：

> `ui_login` is a new JWT-based authentication scheme, which tightly integrates with the chosen UI. Instead of restricting access completely, this allows the user to still open the UI (e.g. WebUI). Unlike the other two modes, this means the login page is entirely customizable by the UI.

**上游是两层，本仓库只有第一层**：

| 层 | 位置 | 作用 |
| --- | --- | --- |
| 1 中间件 | `JavalinSetup.beforeMatched` | 页面级门禁：`SIMPLE_LOGIN` 303、`BASIC_AUTH` 401。`UI_LOGIN` 在此**没有任何分支** |
| 2 端点 | 每个 handler 的 `ctx.getAttribute(TachideskUser).requireUser()` | 数据授权；`Visitor` → 401。唯一例外是 `MangaController.pageRetrieve` 在 `?opds=true` 时用 `requireUserWithBasicFallback`，让 OPDS 阅读器用 Basic 取页图 |

全仓没有端点层，也没有主体概念（无 `require_user`）。上游的 `&& !isApi` 原意只是"API 不重定向"，被搬成 `cookie_ok || path.starts_with("/api/")` 后语义倒转成"API 完全不设防"（S3）——所以 S3 不能靠中间件单点收敛，必须先把端点层建起来。

**UI_LOGIN 为什么不能整站拦截**：WebUI 的登录页由 SPA 自己渲染，且**由 401 触发**（`BaseClient.ts:70` 收到 401 → `setAuthRequired(true)` → `LoginPage`），不是读 `authMode` 得出的。任何"整站拦截 + 服务端登录页"的模式都无法给 WebUI 提供登录入口。`UI_LOGIN` 的"不整站拦截"是这套前端的硬要求，不是宽松。

**重复度**：只有一对真正接近——`BASIC_AUTH` 与 `SIMPLE_LOGIN` 的门禁范围与凭据语义相同（单用户名 + 密码），差别只在登录仪式与凭据生命期（每请求 vs cookie / 30 分钟）。对 WebUI 用户而言 `SIMPLE_LOGIN` 与 `UI_LOGIN` 也是两种重复的登录页，只是渲染方不同。

**设置页接上**（F4）：启动时把 blob 里的 `authMode`/`authUsername`/`authPassword` 作为**默认值**读入，env 优先——与 `load_data_dir_setting` 完全同构（`lib.rs:309`）。改完重启生效，不引入 `Arc<RwLock<ServerConfig>>` 这种全局改动。

## 6. 实施阶段

### 阶段 1 —— 堵住未授权数据访问（最高优先级，可独立发布）

1. 新增 `safe_join`（§3.4），替换 `webui_fallback`、`local_file` 的路径解析；`extension.rs` 的 `pkg` 加字符集校验。
2. 重写豁免判定为 §3.3；删除 `/api/**` 放行（S3）、`ends_with` 白名单（S5）、`/api/v1/shutdown` 豁免（S9）。
3. 401/303 分流（§3.2）。
4. `AuthMode::from_str` 返回 `Result`，未识别值启动失败（S6）。
5. cookie 校验换成签名校验（S4）——此步依赖 §4 的密钥，若与阶段 2 一起做可先留 TODO 但**必须同批发布**，否则 S4 仍在。

**验收**：§7 全表。

### 阶段 2 —— 打通 SIMPLE_LOGIN 登录闭环

6. `GET /login.html`：返回内联样式的最小 HTML 表单（零外部资源，不依赖 `/assets/*`），接受 `?redirect=` 与 `?error=`。
7. `POST /login.html`（表单字段 `user`/`pass`）：
   - 成功 → `Set-Cookie: logged-in=…` → `303` 到 `redirect`
   - `redirect` 只接受相对路径：拒绝 `//`、含 `:`、含 `\` 的值，否则回退 `/`
   - 失败 → `200` 重渲染 + 错误文案（不设 cookie）
8. `GET /logout`：清 cookie → `303 /login.html`。
9. `/api/` 的 401 响应体带 `{"error":"unauthorized"}`，页面请求 303。

**验收**：把 issue #5 的复现路径转成回归脚本（`/` → 303 → 登录页 → POST → 303 → SPA 加载 → `/assets/*` 200）。

### 阶段 3 —— UI_LOGIN / JWT

WebUI 原生的登录形态：401 → SPA 登录页 → `login` mutation → 持令牌重试。`AuthManager`、`LoginPage`、`refreshToken` 这条链路 WebUI 侧已经写好，服务端侧是空桩（F3）。它也是唯一需要端点层（§5）才能成立的安全模式。

10. 实现 `login` / `refreshToken`（HS256，密钥复用 §4；`jwtTokenExpiry`/`jwtRefreshExpiry`/`jwtAudience` 已在 settings 中）。
11. 中间件接受 `Authorization: Bearer`；`UI_LOGIN` 下**不对页面做整站拦截**（否则 WebUI 加载不出登录页，见 §5）。
12. WS 认证：`graphql-transport-ws` 的 `connection_init` payload 携带 token（WebUI 侧 `graphql-ws` 客户端需同步改）。

### 阶段 4 —— 客户端与兼容

13. 实测 BASIC_AUTH 下浏览器 WS 是否携带凭据；不行则 WS 退化为"同源 + 签名 cookie"或允许子协议传 token。
14. `docs/user-guide.md` 认证章节重写：模式表 + 各模式凭据通道 + 从旧版迁移说明。
15. WebUI 配合：401 → 跳登录页；设置页加登出；认证相关文案对齐。

## 7. 验收矩阵

| 请求 | 期望 |
| --- | --- |
| `GET /`（匿名，SIMPLE_LOGIN） | 303 `/login.html?redirect=/` |
| `GET /`（匿名，BASIC_AUTH） | 401 + `WWW-Authenticate: Basic` |
| `GET /assets/index-*.js`（匿名） | **200**（否则白屏） |
| `GET /favicon.svg`、`/sw.js`（匿名） | 200 |
| `GET /login.html`（匿名） | 200 登录页 |
| `POST /login.html`（正确凭据） | 303 + `Set-Cookie`，随后 `GET /` 200 |
| `POST /login.html`（错误凭据） | 200 + 错误文案，无 `Set-Cookie` |
| `POST /login.html`（`redirect=//evil.com`） | 303 到 `/`，不跳外站 |
| `GET /`（`Cookie: logged-in=x` 伪造） | 303 登录页 |
| `GET /`（cookie 签名过期的） | 303 登录页 |
| `GET /api/graphql`（匿名） | 401 |
| `POST /api/graphql`（匿名） | 401 |
| `WS /api/graphql`（匿名） | 握手拒绝 |
| `GET /api/v1/backup/export`（匿名） | 401 |
| `GET /api/v1/backup/export/file`（匿名） | 401 |
| `GET /api/v1/local/<...>`（匿名） | 401 |
| `GET /local/<...>`（匿名） | 401 |
| `GET /api/opds/v1.2/`、`.../library/series`（匿名） | 401 |
| `GET /api/v1/extension/icon/{pkg}`（匿名） | 401 |
| `GET /api/v1/image/{b64}`（匿名） | 401 |
| `GET /../db/suwayomi.db` | **400 或 404**，绝不 200 |
| `GET /C:/Windows/win.ini` | **400 或 404** |
| `GET /..%2f..%2fetc%2fpasswd` | 400 或 404 |
| `GET /local/C:/Windows/win.ini` | 400 或 404 |
| `GET /api/v1/extension/icon/..%2F..%2Fsecret` | 400（pkg 非法） |
| `POST /api/graphql` 带有效 cookie + `Origin: https://evil` | 403 |
| `SUWAYOMI_AUTH_MODE=FOO` 启动 | 进程启动失败，日志列出允许值 |
| `SUWAYOMI_AUTH_MODE` 未设 | 一切不变（回归套件全绿） |

## 8. 待裁决

| # | 决策 | 建议 |
| --- | --- | --- |
| A | 是否支持 `?token=` / `?apikey=` 查询参数凭据（方便 KOReader/脚本，但会进日志与浏览器历史） | **不支持**。KOReader 支持 Basic，够用 |
| B | 设置页的 `authMode`：接成"env 优先、blob 兜底、重启生效"，还是仅改文案声明"由环境变量决定" | **接上**（与 `dataDir` 同构，改动小）；热更新不做 |
| C | 是否本轮实现 UI_LOGIN/JWT | **与阶段 2 并列而非推后**。`SIMPLE_LOGIN` 覆盖"非 WebUI / 无 JS"的浏览器，`UI_LOGIN` 是 WebUI 唯一原生形态（401 → SPA 登录页）；两者共用同一套主体模型与签名密钥，可同批发布 |
| D | 是否引入多用户/角色 | **不引入**，保持上游单用户模型 |
| E | `DISABLED` 下是否仍要求 `Authenticated` 才可访问 `/api/**` | 否。DISABLED 语义就是无认证，但启动必须打 warning |

## 9. 兼容性与风险

- **破坏性（预期内）**：SIMPLE_LOGIN 用户在阶段 1 后会立刻发现 `/api/**` 需要凭据——这正是要堵的洞。使用该模式且把 API 暴露到公网的用户，升级后 API 客户端需补 Basic 凭据或改用 cookie。
- **无感**：默认 `DISABLED` 部署行为不变（除 §3.4 的路径穿越修复，那本来就该 404）。
- **第三方客户端**：Basic 通道与凭据语义（用户名 == `auth_username`）不变；`/api/v1/local/**` 从"SIMPLE_LOGIN 下匿名可读"变为需凭据。
- **最大不确定项**：BASIC_AUTH 下浏览器 WebSocket 的凭据携带（阶段 4 实测）。若不带，WebUI 订阅在 Basic 模式下会失效——需与阶段 2 的 cookie 通道二选一或并用以兼容。
- **会话密钥落盘**：`session.key` 与 db 同目录；§3.4 修好前该文件可被任意文件读取漏洞读走（阶段 1 先修，阶段 2 才落密钥，顺序不能颠倒）。

## 10. 提交切分

| 提交 | 内容 |
| --- | --- |
| 1 | 阶段 1：`safe_join` + 豁免重写 + 401/303 分流 + 模式解析失败即退（安全修复，独立可发布） |
| 2 | 阶段 2：`/login.html` GET/POST + `/logout` + 签名 cookie + 密钥管理 |
| 3 | 阶段 2 配套：WebUI 401 处理、登出、文案；`docs/user-guide.md` 认证章节 |
| 后续 | 阶段 3（JWT）、阶段 4（WS 实测 & 客户端兼容）按需单独提 |
