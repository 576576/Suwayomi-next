# Suwayomi-next

[简体中文](../../README.md) | English

![128x128](../../assets/images/128x128.png)

A Rust implementation of the [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server/), fully compatible with the existing Tachiyomi data model, the GraphQL/REST/OPDS APIs and the Mihon extension ecosystem — everything except the extension runtime layer is written in Rust. This project is a reference implementation of the API, not a fork of the original [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server).

![page-setting](../../assets/screenshots/page-setting.png)

## What's implemented

Implemented: core data model & database layer, business logic (domain),
REST API v1, GraphQL API, OPDS, downloads / library updates / backups / trackers
/ KOReader·SyncYomi sync, a JVM extension sandbox, a Tauri desktop shell and
the release CI. REST v1 and GraphQL schema baselines live in `docs/api/` and
`docs/graphql/`; behaviour is compatible with the original Suwayomi.

## Quick start (from source)

```bash
# Build & run (default port 8090, embedded Oliphaunt PostgreSQL 18, zero
# external dependencies; falls back to a higher port if 8090 is taken)
cargo run --release -p suwayomi-server
```

- WebUI: `http://localhost:8090`
- GraphQL: `/api/graphql` · REST: `/api/v1` · OPDS: `/api/opds/v1.2` (KOReader)
- Full configuration: **`../user-guide.md`**

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
  ├─ jvm-sandbox.jar   extension sandbox (JVM)
  └─ extensions/       converted jars of installed extensions (auto)
data/                 default data dir (Tachiyomi compatible)
webui/                Suwayomi-WebUI bundle (attached per release)
jre/   oliphaunt-runtime/   runtime dependencies (optional)
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
4. **Install extensions**: the APK is downloaded into `extensions/`, converted
   by the JVM sandbox (dex2jar) and loaded; the converted jar lands in
   `bin/extensions/` and the sources are registered in the database. Uninstall
   cleans up both.
5. **Port**: defaults to 8090 with automatic fallback when occupied; when used
   with the desktop shell, the tray settings take precedence.
6. **Logs**: `cache/logs/` holds `server.log`, `tray.log`, `sandbox.log` —
   check these first when debugging.

## Repository layout

```
crates/
  suwayomi-core/     domain models + schema + database layer
  suwayomi-domain/   business logic
  suwayomi-rest/     REST API v1
  suwayomi-graphql/  GraphQL API
  suwayomi-opds/     OPDS
  suwayomi-server/   server entry point
jvm-sandbox/         extension sandbox (Kotlin: AndroidCompat + dex2jar +
                     ChildFirstClassLoader)
suwayomi-tray/       desktop shell (Tauri 2; separate workspace, not part of
                     the main workspace; Windows/Linux)
tools/h2-dump/       H2 → PostgreSQL migration tool (Kotlin)
migrations/          SQL migrations (incl. pg-only/: SyncYomi triggers)
scripts/             CI/helper scripts (resolve-webui.sh / unzip_any.py, …)
assets/              icons & screenshots (images/, screenshots/)
docs/                docs (api/, graphql/, migration/, en/, release.md,
                     user-guide.md)
```

## Database backends

- **Default**: embedded Oliphaunt (native PostgreSQL 18, data in `./pglite-data`)
- **External**: set `SUWAYOMI_DATABASE_URL`, e.g.
  `postgres://user:pass@host:5432/db`

## Real extensions (JVM sandbox)

The server can spawn a JVM sandbox process that drives real Mihon/Tachiyomi
extensions over an HTTP contract (APK → dex2jar → ChildFirst class loading +
reflection):

```bash
# 1) Build the sandbox (JDK 25 toolchain; output
#    build/libs/suwayomi-jvm-sandbox.jar)
cd jvm-sandbox
gradle build          # needs jvm-sandbox/libs/AndroidCompat-1.0.jar (build it
                      # from Suwayomi-Server's AndroidCompat module and copy it,
                      # or substitute an equivalent Android stub)
cd ..

# 2) Put extension APKs into a directory (default ./extensions, or set
#    SUWAYOMI_EXTENSIONS_DIR)
# 3) Start the server with the sandbox enabled
SUWAYOMI_SANDBOX_JAR=jvm-sandbox/build/libs/suwayomi-jvm-sandbox.jar \
SUWAYOMI_SANDBOX_PORT=8091 \
SUWAYOMI_EXTENSIONS_DIR=/path/to/extensions \
SUWAYOMI_SANDBOX_PROXY=127.0.0.1:7890 \   # optional: HTTP proxy
./target/release/suwayomi-server
```

Environment: `SUWAYOMI_SANDBOX_JAR` (enables the sandbox),
`SUWAYOMI_SANDBOX_PORT` (default 8091), `SUWAYOMI_EXTENSIONS_DIR` (default
`./extensions`), `SUWAYOMI_JAR_DIR` (converted-jar dir, default
`<extensions>/../bin/extensions`), `SUWAYOMI_SANDBOX_PROXY` (optional HTTP
proxy). Without a configured sandbox the server falls back to the built-in
`StubFetcher`.

## Extension installs & source management

Extensions install online from **repo indexes** and their sources are
registered in the database automatically, shared across the UI and the API:

- **Repos**: `extension_store` keeps the `index_url` (supports the v1 array and
  the keiyoushi v2 object formats). `POST /api/v1/extension/refresh` (or
  GraphQL `fetchExtensions`) fetches the index and upserts the `extension`
  table (apkUrl/version/NSFW etc.). Indexes are cached under
  `extensions/index/{repo}/index.pb|json` and fall back to the cache when the
  repo is unreachable.
- **Install/update/uninstall**: `GET /api/v1/extension/{install|update|uninstall}/{pkgName}`
  (GraphQL `updateExtension`/`updateExtensions` patches). Installing downloads
  the APK into `SUWAYOMI_EXTENSIONS_DIR` (default `./extensions`, named
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
- **Version triggers**: `migrations/pg-only/0002_*` bump row versions on
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

- `../user-guide.md` — user guide (configuration/backup/OPDS/Docker)
- `../release.md` — release pipeline & CI conventions
- `../migration/MIGRATE.md` — migrating from the Kotlin version
- `../api/rest-endpoints-baseline.md` — REST v1 baseline
- `../graphql/README.md` — GraphQL schema baseline

## Docker

```bash
docker build -t suwayomi-next .
docker run -p 8090:8090 -v suwayomi-data:/data suwayomi-next   # 8090 on both
```

## License

Mozilla Public License, v.2.0

    Copyright (C) Contributors to the Suwayomi project
    
    This Source Code Form is subject to the terms of the Mozilla Public
    License, v. 2.0. If a copy of the MPL was not distributed with this
    file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Disclaimer

The developer of this application does not have any affiliation with the content providers available.
