# Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

**The certificate is issued to SignPath Foundation, not to this project** — that is the
publisher name the installer wizard and SmartScreen show.

## Roles

| Role | Member |
|---|---|
| Authors | [@576576](https://github.com/576576) |
| Reviewers | [@576576](https://github.com/576576) |
| Approvers | [@576576](https://github.com/576576) |

## What gets signed

Only binaries built from this repository's own source:

- `suwayomi.exe` — the tray shell;
- `bin/suwayomi-server.exe` — the server.

Third-party binaries shipped alongside are **not** signed, and are redistributed as-is —
in particular the Temurin JRE under `jre/`.

This covers the Windows installer (`msi`, `setup.exe`) and, later, the `msix` package.
Artifacts published before signing is wired up are unsigned — currently true for `msi` and
`setup.exe`. Releases are at
[github.com/576576/Suwayomi-next/releases](https://github.com/576576/Suwayomi-next/releases).

## Approval

Every signing request is approved by hand: an Approver reviews the build and authorizes it on
the SignPath console. There is no unattended signing pipeline — a push may produce unsigned
artifacts automatically, but signing one always takes a human decision. All members above have
MFA enabled on GitHub and on SignPath.

## Privacy

See [PRIVACY.md](PRIVACY.md). The program does not transfer any information to other networked
systems unless specifically requested by the user or the person installing or operating it.

## 中文

Windows 产物（`msi` / `setup.exe`，以及后续的 `msix`）走 [SignPath.io](https://signpath.io/)
提供的免费开源签名，证书由 [SignPath Foundation](https://signpath.org/) 签发。

**证书签发对象是 SignPath Foundation 而不是本项目** —— 安装向导与 SmartScreen 里显示的
发布者因此是 SignPath Foundation。

| 角色 | 成员 |
|---|---|
| Authors | [@576576](https://github.com/576576) |
| Reviewers | [@576576](https://github.com/576576) |
| Approvers | [@576576](https://github.com/576576) |

被签的只有本项目自源码构建的产物：`suwayomi.exe`（托盘外壳）与 `bin/suwayomi-server.exe`。
随包分发的第三方二进制不在签名范围内、原样分发，典型如 `jre/` 下的 Temurin。签名接入之前
发布的产物是未签名的（`msi` / `setup.exe` 目前如此），产物见
[Releases](https://github.com/576576/Suwayomi-next/releases)。

每一次签名请求都由 Approver 在 SignPath 控制台上人工批准，没有无人值守的签名流水线：
自动构建可以照常出未签名产物，但签名那一步必须有人点。以上成员在 GitHub 与 SignPath
两侧都开了 MFA。

隐私见 [PRIVACY.md](PRIVACY.md)：本程序不会向你或任何第三方传输信息，除非你（安装或使用
它的人）明确要求 —— 所有对外请求都由你的操作触发。
