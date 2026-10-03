# Suwayomi-next

[简体中文](../../README.md) | English

![128x128](../../assets/images/icons/128x128.png)

Suwayomi-next is a manga reader project with a plugin system based on Tachiyomi extensions, offering a modern, cross-platform reading experience — both as an all-in-one desktop client and as a separated server / client. Its implementation references [Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server) and [Mihon](https://github.com/mihonapp/mihon).

![page-setting](../../assets/images/page-setting.png)

## Usage

1. Run `suwayomi` (`suwayomi.exe` on Windows)
2. The system WebView window opens automatically, and the following endpoints are available:

- WebUI: `http://localhost:4567` (falls back to a higher port when 4567 is unavailable)
- GraphQL: `/api/graphql`
- REST: `/api/v1`
- OPDS: `/api/opds/v1.2`, `/api/opds/v2`

> The desktop WebUI window is opened by the **system WebView** (Tauri 2); without a WebView it falls back to the browser
>
> Full configuration reference: **`../zh/user-guide.md`**.

## Features

| Feature | Description |
| --- | --- |
| Tachiyomi/Mihon compatible | Ships the exact same storage / `.tachibk` backup format, interoperable with Tachiyomi/Mihon |
| Suwayomi compatible | Works with the other servers/clients under the Suwayomi Project, interoperable both ways |
| Smaller footprint | Deep analysis and trimming: 40%+ smaller than Suwayomi-Server, lower memory usage |
| Memory safe | Built on the memory-safety of Rust/Kotlin, following best practices |
| Modern UX | Desktop tray, operation gestures and other quality-of-life features |

## Project structure

The project is split into 3 repositories for auditability and shorter CI runs, and bundles the artifacts of 1 further repository as the default frontend:

| Component | Repository |
| --- | --- |
| Server | [Suwayomi-next](#Suwayomi-next) |
| Tray | [Suwayomi-tray](https://github.com/576576/Suwayomi-tray) |
| Extension runtime | [Suwayomi-ext-runtime](https://github.com/576576/Suwayomi-ext-runtime) |
| Default frontend | [576576/Suwayomi-WebUI](https://github.com/576576/Suwayomi-WebUI) |

The component structure of the repositories:

![Suwayomi-next component structure](../../assets/images/project-struct-simple.png)

The artifact directory layout:

```
suwayomi              desktop tray
bin/                  release binaries
  ├─ suwayomi-server    server
  └─ ext-runtime.jar    extension runtime
webui/                Suwayomi-WebUI build output (attached per release)
jre/                  trimmed JRE (attached per release, used by ext-runtime.jar)

<data>/               data directory (Tachiyomi compatible)
  local/              local sources
  downloads/          downloads from source extensions
  autobackup/         automatic backup directory

<appdata>/            application data directory
  extensions/{apk,bin}  extension APKs and dex2jar output
  settings/           tracker credentials and source preferences
  cache/              manga image / thumbnail / repo index / cover caches
  logs/               server / tray / sandbox log files
  db/                 suwayomi.db and session.key
```

## Building from source

### GitHub Actions

GitHub Releases provide packages that run right after unpacking / installing.
To build a different combination (platform / architecture / extra options):

1. Fork this repository

2. Run the `Release` workflow manually from the Actions page

3. Tick the target platforms, channel and configuration options

4. Download the matching artifacts from the Release page once the run finishes

   > Note: the Setup exe/msi(x) in artifacts you build yourself does not carry the repository-specific signing; you may need to configure your own

### Local build

#### Windows

> Supported architectures: `x64` | `arm64`

Environment requirements:

- **Rust**: stable toolchain, 1.91+

  - Host target `x86_64-pc-windows-msvc` | `aarch64-pc-windows-msvc`

    Visual Studio Build Tools: provides the MSVC linker and the Windows SDK

  - Host target `x86_64-pc-windows-gnullvm` | `aarch64-pc-windows-gnullvm`

    LLVM/Clang toolchain: provides llvm-mingw or MSYS2 CLANG64

- **Git**: for cloning the repositories and for Git Bash

- **JDK 25+**: for building the extension runtime and the trimmed JRE

- **Node 24.20+, pnpm 11**: for building the WebUI

```bash
# use Git Bash (or any bash-compatible shell)
git clone https://github.com/576576/Suwayomi-next         Suwayomi-next
git clone https://github.com/576576/Suwayomi-tray         tray
git clone https://github.com/576576/Suwayomi-ext-runtime  ext-runtime
git clone https://github.com/576576/Suwayomi-WebUI        WebUI

# all four components are fixed parts of the release layout; miss one and the layout can't be assembled
cd Suwayomi-next  && cargo build --release -p suwayomi-server  # → target/release/suwayomi-server.exe
cd ../tray        && cargo build --release                     # → target/release/suwayomi.exe
cd ../ext-runtime && ./gradlew jar                             # → build/libs/ext-runtime.jar
cd ../WebUI       && pnpm i && pnpm build                      # → build/

# assemble into the release layout; the final artifact directory is Suwayomi-build
cd ../
mkdir -p Suwayomi-build/bin
cp Suwayomi-next/target/release/suwayomi-server.exe Suwayomi-build/bin
cp tray/target/release/suwayomi.exe Suwayomi-build/
cp ext-runtime/build/libs/ext-runtime.jar Suwayomi-build/bin
cp -r WebUI/build Suwayomi-build/webui
cp -r Suwayomi-next/assets/templates/directory/. Suwayomi-build/ && find Suwayomi-build -name .gitkeep -delete

# optional: build a trimmed jre — x64|aarch64
bash ext-runtime/scripts/make-jre.sh windows x64 Suwayomi-build/jre
ls Suwayomi-build/
```

The repo-root **`build.bat`** is the one-shot version of the above: it does **not** build the other
three repositories, it pulls their Release assets to assemble them (needs cargo / git / curl /
Python / PowerShell only — no JDK or Node), and the artifacts land in `target\artifacts\`.

#### Linux

> Supported architectures: `x64` | `arm64`

Environment requirements:

- **Rust**: stable toolchain, 1.91+
  - Host targets: `x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu`
- **Build deps**: `pkg-config`, `libssl-dev`, `perl` — `perl` is used by the build scripts
- **Tray deps**: `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libxdo-dev`
- **Git**: for cloning the repositories
- **JDK 25+**: for building the extension runtime and the trimmed JRE
- **Node 24.20+, pnpm 11**: for building the WebUI

```bash
# system deps (Debian/Ubuntu package names below)
sudo apt-get install -y pkg-config libssl-dev perl \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev

git clone https://github.com/576576/Suwayomi-next         Suwayomi-next
git clone https://github.com/576576/Suwayomi-tray         tray
git clone https://github.com/576576/Suwayomi-ext-runtime  ext-runtime
git clone https://github.com/576576/Suwayomi-WebUI        WebUI

# all four components are fixed parts of the release layout; miss one and the layout can't be assembled
cd Suwayomi-next  && cargo build --release -p suwayomi-server  # → target/release/suwayomi-server
cd ../tray        && cargo build --release                     # → target/release/suwayomi
cd ../ext-runtime && ./gradlew jar                             # → build/libs/ext-runtime.jar
cd ../WebUI       && pnpm i && pnpm build                      # → build/

# assemble into the release layout; the final artifact directory is Suwayomi-build
cd ../
mkdir -p Suwayomi-build/bin
cp Suwayomi-next/target/release/suwayomi-server Suwayomi-build/bin
cp tray/target/release/suwayomi Suwayomi-build/
cp ext-runtime/build/libs/ext-runtime.jar Suwayomi-build/bin
cp -r WebUI/build Suwayomi-build/webui
cp -r Suwayomi-next/assets/templates/directory/. Suwayomi-build/ && find Suwayomi-build -name .gitkeep -delete

# optional: build a trimmed jre — x64|aarch64
bash ext-runtime/scripts/make-jre.sh linux x64 Suwayomi-build/jre
ls Suwayomi-build/
```

#### macOS

> Supported architectures: `arm64` | @Deprecated `x64`

Environment requirements:

- **Rust**: stable toolchain, 1.91+
  - Host targets `aarch64-apple-darwin` | @Deprecated `x86_64-apple-darwin`
- **Xcode Command Line Tools**: provides clang and the macOS SDK (`xcode-select --install`)
- **Git**: for cloning the repositories
- **JDK 25+**: for building the extension runtime and the trimmed JRE
- **Node 24.20+, pnpm 11**: for building the WebUI

```bash
git clone https://github.com/576576/Suwayomi-next         Suwayomi-next
git clone https://github.com/576576/Suwayomi-tray         tray
git clone https://github.com/576576/Suwayomi-ext-runtime  ext-runtime
git clone https://github.com/576576/Suwayomi-WebUI        WebUI

# all four components are fixed parts of the release layout; miss one and the layout can't be assembled
cd Suwayomi-next  && cargo build --release -p suwayomi-server  # → target/release/suwayomi-server
cd ../tray        && cargo build --release                     # → target/release/suwayomi
cd ../ext-runtime && ./gradlew jar                             # → build/libs/ext-runtime.jar
cd ../WebUI       && pnpm i && pnpm build                      # → build/

# assemble into the release layout; the final artifact directory is Suwayomi-build
cd ../
mkdir -p Suwayomi-build/bin
cp Suwayomi-next/target/release/suwayomi-server Suwayomi-build/bin
cp tray/target/release/suwayomi Suwayomi-build/
cp ext-runtime/build/libs/ext-runtime.jar Suwayomi-build/bin
cp -r WebUI/build Suwayomi-build/webui
cp -r Suwayomi-next/assets/templates/directory/. Suwayomi-build/ && find Suwayomi-build -name .gitkeep -delete

# optional: build a trimmed jre — x64|aarch64
bash ext-runtime/scripts/make-jre.sh mac aarch64 Suwayomi-build/jre
ls Suwayomi-build/
```

#### Android

> Supported architectures: `x64` | `arm64`

Environment requirements:

- **Rust**: stable toolchain, 1.91+
  - Android targets (`rustup target add aarch64-linux-android x86_64-linux-android`)
- **Android SDK**: `platforms;android-37.0` and `ndk;28.2.13676358`
  - Point the `ANDROID_HOME` / `ANDROID_NDK_HOME` environment variables at them
- **JDK 25+**: Gradle 9 and AGP 9.2.1
- **Node 24.20+, pnpm 11**: for building the WebUI
- **Git, python3, curl**: for fetching the shared extension sources and packaging assets
- The two cross-compilation pitfalls and the toolchain version table are in **`../agent/android.md`**

```bash
# first prepare a WebUI build output at WebUI/, next to Suwayomi-next
# (package-webui.sh also accepts a webui zip downloaded from Releases)
git clone https://github.com/576576/Suwayomi-WebUI ../WebUI
(cd ../WebUI && pnpm i && pnpm build)

# ABI=arm64|x86_64|all
ABI=arm64 bash android/scripts/build-rust.sh      # server → cdylib → app/src/main/jniLibs/arm64-v8a/
bash android/scripts/fetch-ext-runtime-src.sh     # downloads the ext-runtime sources shared by :extension-host
bash android/scripts/package-webui.sh ../WebUI/build  # WebUI output → app/src/main/assets/webui.zip
cd android && ./gradlew :app:assembleRelease --no-daemon
# → android/app/build/outputs/apk/release/app-release.apk
```

## Repository layout

```
crates/              the Rust crates (layering in the structure diagram above)
android/             Android host project (separate Gradle/AGP build, not merged into the main project)
scripts/             CI/helper scripts (resolve-webui.sh / unzip_any.py, …)
assets/              images (images/: icons, screenshots, structure diagram) and directory-tree templates (templates/)
docs/                docs (zh/ Chinese, en/ English, agent/ for maintainers and AI)
```

## Optional environment variables

| Variable | Default | Description |
| --- | --- | --- |
| `SUWAYOMI_APPDATA_DIR` | `appdata/` next to the exe | The **single writable root** for the program's own state: `cache/`, `db/`, `settings/`, `extensions/{apk,bin}` all live under it, each with no directory variable of its own |
| `SUWAYOMI_DATA_DIR` | `data/` next to the exe | User data root: `downloads/`, `local/`, `autobackup/`; also editable in the WebUI under Data & Storage → Storage location |
| `SUWAYOMI_PORT` | `4567` | HTTP port; falls back to a higher one when occupied or when it lands in the Windows dynamic-reservation range |
| `SUWAYOMI_IP` | `0.0.0.0` | Listen address; set it to `127.0.0.1` to allow local access only |
| Other variables | — | See [`docs/zh/user-guide.md`](../zh/user-guide.md): WebUI directory, database backend, sandbox, auth, JWT, … |

## Database backends

- **Default**: the bundled `rheos-tokio-rusqlite` crate
  - `<appdata>/db/suwayomi.db`
- **Alternative**: an external PostgreSQL connection
  - `SUWAYOMI_DB_BACKEND=postgres` (**only this selects the backend**; any other value falls back to SQLite)
  - `SUWAYOMI_DB_URL=postgres://user:pass@host:5432/db` (only says where PostgreSQL is; setting it does not switch the backend)

Everything the program produces for itself (caches, database, settings, extension APKs and their
converted artifacts) lives under a **single writable root**, `SUWAYOMI_APPDATA_DIR` (referred to as
`<appdata>` below); these four have no directory variables of their own:

```
<appdata>/
  cache/     manga image / thumbnail / repo index / cover caches
  logs/      server / tray / sandbox log files
  db/        suwayomi.db and session.key
  settings/  tracker credentials and source preferences
  extensions/{apk,bin}   extension APKs and dex2jar output
```

## Key docs

- `../zh/user-guide.md` — user guide (configuration/backup/OPDS/Docker)
- `../agent/release.md` — release pipeline & CI conventions
- `../agent/rest-api.md` — REST v1 endpoint compatibility baseline
- `../agent/graphql.md` — GraphQL schema baseline
- `../agent/android.md` — Android host project (build & verification)

## Docker

The release pipeline pushes images to GHCR, using the same tag rules as artifact names: `r<commit count>`
for alpha, `3.y.z` for release/beta (beta additionally carries `-beta`). **The release / beta channels
also tag `latest`**, alpha does not — it is a pre-release produced on every push and has no business
holding `latest`.

```bash
docker run -p 4567:4567 -v suwayomi-data:/data ghcr.io/576576/suwayomi-next:latest
```

## Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

The Windows installer artifacts of this project (`msi` / `setup.exe` / later `msix`) are signed by
[SignPath.io](https://signpath.io/) free of charge, with the certificate issued by
[SignPath Foundation](https://signpath.org/).

| Role | Member |
|---|---|
| Authors | [@576576](https://github.com/576576) |
| Reviewers | [@576576](https://github.com/576576) |
| Approvers | [@576576](https://github.com/576576) |

Every signing request is approved by hand by an Approver on the SignPath console; there is no unattended
signing pipeline. Only artifacts built from this project's own source are signed (`suwayomi.exe` and
`bin/suwayomi-server.exe`); third-party binaries shipped alongside (`jre/`, the Temurin OpenJDK) are not
covered.

Privacy: see [PRIVACY.md](../../PRIVACY.md) — the program will not transfer any information to you or to
any third party unless you (the person installing or using it) explicitly ask it to; every outbound
request is triggered by your own action.

Artifacts published before signing is wired up are unsigned (currently true for `msi` / `setup.exe`);
see [Releases](https://github.com/576576/Suwayomi-next/releases).

The full policy (roles, signed scope, approval flow) is in
[CODE_SIGNING_POLICY.md](../../CODE_SIGNING_POLICY.md).

## License

Mozilla Public License, v.2.0

    Copyright (C) Contributors to the Suwayomi project
    
    This Source Code Form is subject to the terms of the Mozilla Public
    License, v. 2.0. If a copy of the MPL was not distributed with this
    file, You can obtain one at http://mozilla.org/MPL/2.0/.

## Disclaimer

The developer of this application does not have any affiliation with the content providers available.
