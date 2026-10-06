#!/usr/bin/env python3
"""取件前核对伴生仓（ext-runtime / tray）这次要用的 Release 资产在不在。

主仓的发布捆两个**别的仓库**发布的资产，而那两个仓推 main 的自动 alpha 只出最小发货集
（ext-runtime 只出两份裁剪 JRE；tray 只出 windows-x64 + linux-x64 且恒 msvc）。于是
「全平台 + windows_toolchain=all」这类组合必然缺件，而缺件的表现不是一句报错 —— 是 5~7
个 job 各自 404，外加 publish 被 skip 导致整轮不出 Release。

所以核对收到 prep 里做：按**本次矩阵真正会用到的资产名**逐个比对该 release 的资产清单，
缺件就直接失败并把「去哪个仓、用哪些参数 dispatch」写清楚。

不逐条发 HEAD 请求：同一个 release 的资产在一次 `releases/tags/<tag>` 响应里，比对名字
即可 —— 请求数从 6~8 次降到 1~2 次，也不会被 CDN 的 302 / 限流干扰。

资产的命名口径来自消费方（改了那边要同步改这里）：
  ext-runtime  build.yml 的「打包」步 + android/scripts/fetch-ext-runtime-src.sh
               <base>/ext-runtime-<V>.jar / ext-runtime-<V>-shared-sources.jar
               <base>/ext-runtime-jre-<V>-<os>-<arch>.tar.gz
  tray         build.yml 的「打包」步
               <base>/suwayomi-tray-<V>-<target>[.exe]（gnullvm 再加同名 .dll）
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys

EXT_REPO = "576576/Suwayomi-ext-runtime"
TRAY_REPO = "576576/Suwayomi-tray"
UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)"
DEFAULT_API = "https://api.github.com"
DEFAULT_HTML = "https://github.com"


def flag(name: str, value: str) -> bool:
    return value == "true"


def split_base(base: str) -> tuple[str, str]:
    """把 <repo>/releases/download/<tag> 拆成 (slug, tag)；形状不对时返回空串。"""
    m = re.match(r"https?://github\.com/([^/]+/[^/]+)/releases/download/(.+)$", base or "")
    return (m.group(1), m.group(2)) if m else ("", "")


# --------------------------------------------------------------------------- #
# 取某个 release 的资产名集合
# --------------------------------------------------------------------------- #
def _gh_api(slug: str, tag: str, api_base: str) -> str:
    """首选 gh：它带 GH_TOKEN，5000 次/小时的额度，不会因为跟别的作业共用出口 IP 被限流。"""
    if api_base != DEFAULT_API or not _which("gh"):
        return ""
    try:
        p = subprocess.run(["gh", "api", f"repos/{slug}/releases/tags/{tag}"],
                           capture_output=True, text=True, timeout=60)
    except Exception:
        return ""
    return p.stdout if p.returncode == 0 else ""


def _which(name: str) -> bool:
    for d in (os.environ.get("PATH") or "").split(os.pathsep):
        for ext in ("", ".exe", ".cmd", ".bat"):
            if d and os.path.exists(os.path.join(d, name + ext)):
                return True
    return False


def _curl(url: str) -> str:
    """匿名取数。--noproxy：本机 HTTPS_PROXY 会间歇性失效，CI 上本来也没有代理。"""
    if not _which("curl"):
        return ""
    try:
        p = subprocess.run(["curl", "-sL", "--noproxy", "*", "--retry", "2", "--max-time", "30",
                            "-A", UA, url], capture_output=True, text=True, timeout=90)
    except Exception:
        return ""
    return p.stdout if p.returncode == 0 else ""


def asset_names(slug: str, tag: str, api_base: str, html_base: str) -> set[str] | None:
    """该 release 的资产名集合（三级探测，与 scripts/resolve-*.sh 同构）。

    取不到时返回 None —— 调用方降级成 ::warning::，不能因为「查不到清单」就判「缺件」。
    两个 base 只在测试里被指向本地假服务；默认就是 GitHub。
    """
    raw = _gh_api(slug, tag, api_base) or _curl(f"{api_base}/repos/{slug}/releases/tags/{tag}")
    if raw:
        try:
            doc = json.loads(raw)
        except Exception:
            doc = None
        if isinstance(doc, dict) and isinstance(doc.get("assets"), list):
            names = {a.get("name") for a in doc["assets"] if isinstance(a, dict) and a.get("name")}
            if names:
                return names
    # 3) 匿名 HTML：把 expanded_assets 页里的下载链接抠成资产名
    page = _curl(f"{html_base}/{slug}/releases/expanded_assets/{tag}")
    if page:
        names = set(re.findall(
            rf"/{re.escape(slug)}/releases/download/{re.escape(tag)}/([^\"/?]+)", page))
        if names:
            return names
    return None


# --------------------------------------------------------------------------- #
# 本次矩阵要用到的资产
# --------------------------------------------------------------------------- #
def desktop_targets(raw: str) -> list[dict]:
    try:
        doc = json.loads(raw or "[]")
    except Exception:
        return []
    return [t for t in doc if isinstance(t, dict) and t.get("target")]


def tray_asset(version: str, target: str) -> list[str]:
    """桌面壳资产名：Windows 带 .exe；gnullvm 那份另要一个同名的 WebView2Loader.dll。"""
    name = f"suwayomi-tray-{version}-{target}"
    if target.startswith("windows"):
        both = [name + ".exe"]
        if target.endswith("-gnullvm"):
            both.append(name + ".dll")
        return both
    return [name]


def plan(targets: list[dict], pack_jre: bool, android: list, ext_version: str,
         tray_version: str) -> dict[str, list[tuple[str, str]]]:
    """-> {repo_key: [(资产名, 谁需要它)]}，repo_key 为 'ext' / 'tray'。"""
    need: dict[str, list[tuple[str, str]]] = {"ext": [], "tray": []}
    if ext_version:
        if android:
            need["ext"].append((f"ext-runtime-{ext_version}-shared-sources.jar", "android 取共享源码"))
        if pack_jre:
            # Windows 的 msvc 与 gnullvm 两个矩阵项共用同一份 JRE 资产 —— 按资产名去重。
            seen = set()
            for t in targets:
                name = f"ext-runtime-jre-{ext_version}-{t.get('jre_os')}-{t.get('jre_arch')}.tar.gz"
                if name in seen:
                    continue
                seen.add(name)
                need["ext"].append((name, f"桌面 {t['target']} 的 +jre 包"))
    if tray_version:
        for t in targets:
            for name in tray_asset(tray_version, t["target"]):
                need["tray"].append((name, f"桌面 {t['target']} 的托盘壳"))
    return need


# --------------------------------------------------------------------------- #
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--targets", default="[]", help="桌面矩阵 JSON（含 target / jre_os / jre_arch）")
    ap.add_argument("--android", default="[]", help="Android 矩阵 JSON（非空 = 会跑 android job）")
    ap.add_argument("--pack-jre", default="false")
    ap.add_argument("--pack-installer", default="false", help="pack_msi 或 pack_exe 至少一个为 true")
    ap.add_argument("--ext-base", default="", help="ext-runtime 的资产下载前缀（<repo>/releases/download/<tag>）")
    ap.add_argument("--ext-version", default="")
    ap.add_argument("--tray-base", default="", help="托盘壳的资产下载前缀；空 = 本次没解析到桌面壳")
    ap.add_argument("--tray-version", default="")
    ap.add_argument("--channel", default="alpha")
    ap.add_argument("--windows-toolchain", default="msvc")
    ap.add_argument("--api-base", default=DEFAULT_API, help="仅测试用：把 REST 基址指到本地假服务（同时会让 gh 那级短路）")
    ap.add_argument("--html-base", default=DEFAULT_HTML, help="仅测试用：把 HTML 兜底那级的基址也指过去")
    a = ap.parse_args()

    targets = desktop_targets(a.targets)
    try:
        android = json.loads(a.android or "[]")
    except Exception:
        android = []
    pack_jre = flag("pack_jre", a.pack_jre)
    installer = flag("pack_installer", a.pack_installer)

    ext_slug, ext_tag = split_base(a.ext_base)
    ext_slug = ext_slug or EXT_REPO
    tray_slug, tray_tag = split_base(a.tray_base)
    tray_slug = tray_slug or TRAY_REPO

    # 桌面壳整批缺失（一个 release 都没解析到）是另一回事：不是"某个 target 缺件"，
    # 而是本次包整体不含托盘。要出 Windows 安装包时它必然 ICE64（.wxs 里含 AppMenuFolder
    # 的 RemoveFolder 组件被 HasTray 条件排除）—— 那不是可容忍的降级，直接失败。
    win_targets = [t["target"] for t in targets if t["target"].startswith("windows")]
    tray_absent_hard = bool(targets) and not a.tray_version and installer and win_targets

    need = plan(targets, pack_jre, android, a.ext_version, a.tray_version)
    if not a.tray_version:
        need["tray"] = []
    elif tray_absent_hard:
        need["tray"] = []

    if not need["ext"] and not need["tray"] and not tray_absent_hard:
        if targets and not a.tray_version:
            print("::warning::没解析到任何 Suwayomi-tray Release，本次包不含桌面壳")
        else:
            print("本次没有需要从伴生仓取的资产，跳过覆盖检查")
        return 0

    jobs = [("ext", ext_slug, ext_tag, "ext-runtime"), ("tray", tray_slug, tray_tag, "tray")]
    missing: dict[str, list[tuple[str, str]]] = {}
    verified = 0
    for key, slug, tag, label in jobs:
        if not need[key]:
            continue
        if not tag:
            print(f"::warning::拿不到 {label} 的 release tag，跳过它的覆盖检查")
            continue
        names = asset_names(slug, tag, a.api_base.rstrip("/"), a.html_base.rstrip("/"))
        if names is None:
            # 查不到清单 ≠ 缺件：降级为告警，让下游照旧报它自己的 404。
            print(f"::warning::取不到 {slug}@{tag} 的资产清单（gh / API / HTML 三级均无结果），"
                  f"跳过覆盖检查")
            continue
        verified += 1
        miss = [(n, why) for n, why in need[key] if n not in names]
        print(f"{slug}@{tag}：需要 {len(need[key])} 个，缺 {len(miss)} 个")
        if miss:
            missing[key] = miss

    if tray_absent_hard:
        print("::error::没解析到任何 Suwayomi-tray Release；本次要出 Windows 安装包"
              f"（{'、'.join(win_targets)}），缺托盘壳会让 wix msi validate 报 ICE64")

    if not missing and not tray_absent_hard:
        # 一个清单都没取到时不能说"齐备" —— 那是没查，不是查过。
        print("伴生仓资产齐备" if verified else "覆盖检查未完成（没取到任何资产清单）")
        return 0

    if missing:
        total = sum(len(v) for v in missing.values())
        print(f"::error::伴生仓资产缺件（{total} 个）—— 这些资产在 CI 上一定取不到，先补齐再重跑")
    for key, slug, tag, label in jobs:
        for name, why in missing.get(key, []):
            print(f"::error::{label}@{tag} 缺 {name}（{why}）")

    if a.ext_version and missing.get("ext"):
        print(hint_ext(a.channel))
    if missing.get("tray") or tray_absent_hard:
        print(hint_tray(a.channel, a.windows_toolchain, targets))
    print("提示：勾上 refresh_companions 可以让本 workflow 先派发两仓的对应构建，就不用手动补")
    return 1


def hint_ext(channel: str) -> str:
    return (f"  补齐 ext-runtime：https://github.com/{EXT_REPO}/actions/workflows/release.yml\n"
            f"    Run workflow → channel={channel}，build_jre=true（要六份 JRE），publish_packages=false")


def hint_tray(channel: str, toolchain: str, targets: list[dict]) -> str:
    # 托盘仓的平台开关没有工具链维（工具链是单独一个 input），所以带 -gnullvm 的矩阵项要先
    # 摘掉工具链段再映射回它的平台开关名。
    PLAT = ("windows-x64", "windows-arm64", "linux-x64", "linux-arm64", "macos-x64", "macos-arm64")
    bases = {t["target"][: -len("-gnullvm")] if t["target"].endswith("-gnullvm") else t["target"]
             for t in targets}
    plat = [p for p in PLAT if p in bases]
    return (f"  补齐桌面壳：https://github.com/{TRAY_REPO}/actions/workflows/release.yml\n"
            f"    Run workflow → channel={channel}，windows_toolchain={toolchain}，"
            f"平台勾选：{' '.join(plat) or '（本次矩阵里没有桌面 target）'}")


if __name__ == "__main__":
    sys.exit(main())
