# Privacy Policy · 隐私政策

Suwayomi-next runs entirely on your own machine. It operates no server of ours, and it
collects nothing: no telemetry, no analytics, no crash reports, no account system.

**This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it.**

## What leaves your machine, and when

Every outbound request is triggered by something you do — browsing a source,
downloading a chapter, refreshing the extension list, syncing a tracker. The program
contacts only the following, and only to read public data:

| Recipient | Triggered by | Sent |
|---|---|---|
| Extension repositories you configured | you refreshing the extension list / installing an extension | the request for a public index and APK |
| Manga sites, through the extensions you installed | you browsing, searching, downloading | whatever that extension sends |
| Trackers you linked (MyAnimeList / AniList / Kitsu / …) | you syncing your list | your list data, sent to that tracker — not to us |
| GitHub, for this project's and Suwayomi-WebUI's public release metadata | the WebUI checking for updates, at the interval in `webUIUpdateCheckInterval` (configurable, can be turned off) | a plain read; no user data |

**We operate no server, so no request carries data to us.** There is nothing to opt out
of on our side.

Note that the bundled web service binds `0.0.0.0:4567` by default, so it is reachable
from your local network. Set the bind address to `127.0.0.1` in the server settings if
you do not want that.

## What stays on your machine

Everything else. The library, download queue, reading history, settings, installed
extension APKs and logs live under the two data roots you choose (`appdata/` and
`data/` — see the README). Deleting those directories removes all of it; no copy exists
anywhere else.

If you enable the built-in authentication, the credentials you set are stored on your
own disk and are only used to protect your own instance. They are not an account with
us, because there is no "us".

## Third-party code

Extensions are third-party programs that you install yourself. Their behaviour —
including what they send to the sites they scrape — is outside this project's control.
Each manga site and tracker is governed by its own terms and privacy policy.

## 中文

Suwayomi-next 完全运行在你自己的机器上：我们没有服务端，程序不收集任何东西 ——
没有遥测、没有埋点统计、没有崩溃上报、没有账号系统。

**它不会向其它联网系统传输任何信息，除非你（安装或使用它的人）明确要求。**

所有对外请求都由你的操作触发（浏览图源、下载章节、刷新扩展列表、同步追踪），
只访问下列对象、且只读取公开数据：

- 你配置的**扩展仓库** —— 拉取公开索引与 APK；
- **漫画站点**（经你安装的扩展）—— 发什么由该扩展决定；
- 你绑定的**追踪器**（MyAnimeList / AniList / Kitsu 等）—— 你的列表数据发给该追踪器，不经过我们；
- **GitHub** —— WebUI 检查更新时读取本项目与 Suwayomi-WebUI 的公开发布信息，
  间隔由设置里的 `webUIUpdateCheckInterval` 决定，可关掉。

**我们没有服务端，所以没有任何请求会把数据带到我们这里。**

另外注意：内置的 Web 服务默认绑定 `0.0.0.0:4567`，同一局域网内可访问；不想要的话，
把服务端设置里的监听地址改成 `127.0.0.1`。

其余一切都在你自己的机器上：书库、下载队列、阅读历史、设置、已安装的扩展 APK 与日志，
全部落在你选定的两个数据根（`appdata/` 与 `data/`，见 README）之下，删掉即清除，
别处没有副本。启用内置认证时，凭据存在你自己的磁盘上、只用于保护你自己的实例 ——
它不是我们的账号，因为并不存在「我们」。

扩展是第三方程序，由你自己安装；它们的行为（包括向所抓取站点发送了什么）不受本项目控制。
每个漫画站点与追踪器都受其各自的条款与隐私政策约束。
