# Suwayomi-next

[简体中文](../../README.md) | English

![128x128](../../assets/images/128x128.png)

An independently developed server (Rust) with a desktop tray, referencing [Mihon](https://github.com/mihonapp/mihon) and [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server/): it keeps the existing Tachiyomi data model, the GraphQL/REST/OPDS APIs and the Mihon extension ecosystem compatible, and everything except the extension runtime layer (the JVM sandbox) is written in Rust. This is an independent project, not a fork of [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server).

![page-setting](../../assets/screenshots/page-setting.png)

## What's implemented

Implemented: core data model & database layer, business logic (domain),
REST API v1, GraphQL API, OPDS, downloads / library updates / backups / trackers
/ KOReader·SyncYomi sync, a JVM extension sandbox, a Tauri desktop shell and
the release CI. REST v1 and GraphQL schema baselines live under `docs/agent/`;
behaviour is compatible with Suwayomi-Server.

## Quick start (from source)

```bash
# Build & run (default port 4567, local SQLite database, zero external
# dependencies; falls back to a higher port if 4567 is taken)
cargo run --release -p suwayomi-server
```

- WebUI: `http://localhost:4567`
- GraphQL: `/api/graphql` · REST: `/api/v1` · OPDS: `/api/opds/v1.2` (KOReader)
- Full configuration: **`../zh/user-guide.md`**

## Release package layout & usage

GitHub Releases ship ready-to-run platform archives; the exact platforms and
bundling depend on the targets selected in the manual Release run. To get
artifacts for another platform / bundling combo:

1. Fork this repository
2. Run the `Release` workflow manually from the Actions page
3. Pick the targets and channel (build inputs are one-click configurable)
4. Download the artifacts from the Release page once the run finishes

```
suwayomi              desktop shell (Tauri tray)
bin/
  ├─ suwayomi-server   headless server (single instance)
  ├─ ext-runtime.jar   extension sandbox (JVM)
  └─ extensions/       converted jars of installed extensions (auto)
data/                 default data dir (Tachiyomi compatible)
webui/                Suwayomi-WebUI bundle (attached per release)
jre/                  runtime dependencies (optional)
```

> The bundled WebUI comes from [576576/Suwayomi-WebUI](https://github.com/576576/Suwayomi-WebUI)

The WebUI window is opened through the tray's **system WebView** (Windows
WebView2 / Linux WebKitGTK / macOS WKWebView) — no browser runtime is bundled;
systems without a WebView fall back to the system browser.

### Usage

1. **Launch**: double-click `suwayomi` (silent tray, no terminal window).
   Tray menu:
   - Start / Restart Suwayomi — starts it when not running; shows
     "Restart" while running (graceful shutdown, then relaunch, embedded DB
     included)
   - Open WebUI — system WebView window on `http://127.0.0.1:{port}`
     (falls back to the browser when there is no WebView)
   - Open data dir / Settings (port, data dir, WebUI address; saving restarts
     the server)
   - Exit — ends the tray and the server child process (embedded postgres
     shuts down too)
2. **CLI**: run `bin/suwayomi-server` directly (`-v` prints version & repo).
3. **Add extension repos**: on the WebUI extensions page, add an index URL
   (Mihon `index.pb` or Tachiyomi `index.json`, e.g. keiyoushi), then refresh
   and install extensions online.
4. **Install extensions**: the APK is downloaded into `<appdata>/extensions/apk`,
   converted by the JVM sandbox (dex2jar) and loaded; the converted jar lands in
   `<appdata>/extensions/bin` and the sources are registered in the database.
   Uninstall cleans up both.
5. **Port**: defaults to 4567 with automatic fallback when occupied; the desktop
   shell picks a free port up front (4567 sits in the range Windows sometimes
   reserves for Hyper-V) and its settings take precedence.
6. **Logs**: `<appdata>/logs/` holds `server.log`, `tray.log`,
   `sandbox.log` —
   check these first when debugging.

## Repository layout

```
crates/
  suwayomi-db-macros/ proc-macro (#[derive(FromRow)])
  suwayomi-db/        dual-backend database layer (SQLite default / PostgreSQL;
                      SQL migrations live in its migrations/)
  suwayomi-core/      domain models + table row types + build-time version info
  suwayomi-domain/    business logic (the only layer that speaks HTTP outward)
  suwayomi-api/       shared API layer: AppState + site-wide auth middleware
  suwayomi-rest/      REST API v1
  suwayomi-graphql/   GraphQL API
  suwayomi-opds/      OPDS (KOReader and friends)
  suwayomi-server/    server entry point (assembly + static hosting + JVM sandbox)
  suwayomi-android/   JNI cdylib (ships the whole server inside the Android APK)
android/             Android host project (separate Gradle/AGP build, not
                     merged into the main project)
scripts/             CI/helper scripts (resolve-webui.sh / unzip_any.py, …)
assets/              icons & screenshots (images/, screenshots/)
docs/                docs (zh/ Chinese, en/ English, agent/ for maintainers
                     and AI)
```

The desktop shell (Tauri 2) lives in its own repository,
[576576/Suwayomi-tray](https://github.com/576576/Suwayomi-tray).

## Database backends

- **Default**: a local SQLite file (`appdata/db/suwayomi.db`)
- **External**: set `SUWAYOMI_DB_BACKEND=postgres` plus `SUWAYOMI_DATABASE_URL`,
  e.g. `postgres://user:pass@host:5432/db`

Everything the program writes for itself (cache, database, settings, extension
APK + converted jars) lives under a single writable root, `SUWAYOMI_APPDATA_DIR`
(default `appdata/` next to the executable). There is deliberately **no**
per-directory override for those four.

The database file deliberately lives **outside** the data directory: the data
directory (`SUWAYOMI_DATA_DIR`, also editable in the WebUI under
Settings → Data & Storage → Storage location) is meant to be changed at will,
while the settings themselves are stored in that database.

## Real extensions (JVM sandbox)

The server can spawn a JVM sandbox process that drives real Mihon/Tachiyomi
extensions over an HTTP contract (APK → dex2jar → ChildFirst class loading +
reflection):

The sandbox itself (formerly `jvm-sandbox`, now `ext-runtime`) lives in its own
repository: [576576/Suwayomi-ext-runtime](https://github.com/576576/Suwayomi-ext-runtime).
This repo no longer carries its sources, so locally you just **download a
published jar** — no need to build it yourself. The trimmed JRE used by the
`+jre` desktop bundle and the Docker image is produced there too (jlink, with a
module whitelist that tracks the sandbox code); here we just fetch the asset for
`<version>-<os>-<arch>`, and this repo runs no JVM toolchain at all:

```bash
# 1) Fetch the extension sandbox jar (omit the version to get the latest)
OUT="$(bash scripts/resolve-ext-runtime.sh 30.1.0)"
curl -fsSL -o ext-runtime.jar "$(printf '%s' "$OUT" | sed -n 's/^url=//p')"

# 2) Drop extension APKs into <appdata>/extensions/apk
# 3) Start the server with the sandbox enabled
SUWAYOMI_SANDBOX_JAR=ext-runtime.jar \
SUWAYOMI_SANDBOX_PORT=4568 \
SUWAYOMI_APPDATA_DIR=/var/lib/suwayomi/appdata \
SUWAYOMI_SANDBOX_PROXY=127.0.0.1:7890 \   # optional: HTTP proxy
./target/release/suwayomi-server
```

Environment: `SUWAYOMI_SANDBOX_JAR` (enables the sandbox),
`SUWAYOMI_SANDBOX_PORT` (default 4568), `SUWAYOMI_APPDATA_DIR` (single writable
root — the sandbox child inherits it and derives the extension dir
`extensions/apk`, the converted-jar dir `extensions/bin` and `settings/` from
it), `SUWAYOMI_SANDBOX_PROXY` (optional HTTP proxy). Without a configured
sandbox the server falls back to the built-in `StubFetcher`.

## Extension installs & source management

Extensions install online from **repo indexes** and their sources are
registered in the database automatically, shared across the UI and the API:

- **Repos**: `extension_store` keeps the `index_url` (supports the v1 array and
  the keiyoushi v2 object formats; within v1 the legacy `code`/`version` keys,
  numeric `nsfw` and relative apk paths are accepted too). `POST
  /api/v1/extension/refresh` (or GraphQL `fetchExtensions`) fetches the index
  and upserts the `extension` table (apkUrl/version/NSFW etc.). Indexes are
  cached as `<appdata>/cache/extensions/index/index-<hash>.<pb|json>` (`<hash>`
  of the index URL) and fall back to the cache when the repo is unreachable.
- **Install/update/uninstall**: `GET /api/v1/extension/{install|update|uninstall}/{pkgName}`
  (GraphQL `updateExtension`/`updateExtensions` patches). Installing downloads
  the APK into `<appdata>/extensions/apk` (named
  `tachiyomi-{lang}.{pkg}-v{ver}.apk`), hot-reloads the JVM sandbox (`/reload`)
  and upserts the stable source ids from `/sources` (extension `Source.getId()`)
  into the `source` table.
- **External APKs**: GraphQL `installExternalExtension` (multipart upload) —
  metadata is parsed via the sandbox `/inspect`, then installed.
- **Proxy**: repo/APK downloads reuse the `SUWAYOMI_SANDBOX_PROXY` setting.

## Sync

- **KOReader**: GraphQL `connectKoSyncAccount` / `pushKoSyncProgress` /
  `pullKoSyncProgress` / `koSyncStatus`. Credentials live in `global_meta`; the
  chapter `koreader_hash` is `md5("<manga title> - <chapter name>")`
  (FILENAME checksum).
- **SyncYomi**: GraphQL `startSync` / `lastSyncStatus`. Configure via
  ServerConfig: `syncYomiEnabled` / `syncYomiHost` / `syncYomiApiKey` (plus 6
  `syncData*` scope options and `syncInterval`). Sync uses the Mihon backup
  protobuf with ETag (If-None-Match/If-Match) over
  `{host}/api/sync/content`, doing pull → restore → push.
- **Version triggers**: `crates/suwayomi-db/migrations/{sqlite,postgres}/` bump row versions on
  manga/chapter/category changes (exempting `is_syncing`); applied on both the
  embedded and external PostgreSQL backends.

## Building

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Windows release artifacts (`suwayomi-server.exe` + tray `suwayomi.exe`):
double-click **`build.bat`** in the repo root (or `cmd /c build.bat`).

## Docs

- `../zh/user-guide.md` — user guide (configuration/backup/OPDS/Docker)
- `../zh/migrate-from-kotlin.md` — migrating from the Kotlin version
- `../agent/release.md` — release pipeline & CI conventions
- `../agent/rest-api.md` — REST v1 baseline
- `../agent/graphql.md` — GraphQL schema baseline
- `../agent/android.md` — Android host project

## Docker

```bash
docker build -t suwayomi-next .
docker run -p 4567:4567 -v suwayomi-data:/data suwayomi-next   # 4567 on both
```

## Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

The Windows artifacts (`msi` / `setup.exe`, and later `msix`) are signed through SignPath
Foundation's free signing for open source. **The certificate is issued to SignPath Foundation,
not to this project** — that is the publisher name the installer wizard and SmartScreen show.

| Role | Member |
|---|---|
| Authors | [@576576](https://github.com/576576) |
| Reviewers | [@576576](https://github.com/576576) |
| Approvers | [@576576](https://github.com/576576) |

Every signing request is approved by hand on the SignPath console; there is no unattended
signing pipeline. Only binaries built from this repository's own source are signed
(`suwayomi.exe` and `bin/suwayomi-server.exe`) — third-party binaries shipped alongside
(such as Temurin under `jre/`) are not.

Privacy: see [PRIVACY.md](../../PRIVACY.md). This program will not transfer any information to
other networked systems unless specifically requested by the user or the person installing or
operating it.

Artifacts published before signing is wired up are unsigned (currently true for `msi` /
`setup.exe`); see [Releases](https://github.com/576576/Suwayomi-next/releases). The full policy
(roles, signed scope, approval flow) is in
[CODE_SIGNING_POLICY.md](../../CODE_SIGNING_POLICY.md).

## License

Mozilla Public License, v.2.0

    Copyright (C) Contributors to the Suwayomi project
    
    This Source Code Form is subject to the terms of the Mozilla Public
    License, v. 2.0. If a copy of the MPL was not distributed with this
    file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Disclaimer

The developer of this application does not have any affiliation with the content providers available.
