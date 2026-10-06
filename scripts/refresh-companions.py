#!/usr/bin/env python3
"""先派发伴生仓（ext-runtime / tray）的对应构建，等它跑完，再按 run id 反查 release tag。

主仓一次"全平台 + windows_toolchain=all"的发布要捆两个**别的仓库**发布的资产，而那两个
仓推 main 的自动 alpha 只出最小发货集 —— 缺件的表现是 5~7 个 job 各自 404（裁剪 JRE）
或 ICE64（托盘壳），外加 publish 被 skip，整轮不出 Release。

派发走 `workflow_dispatch`：工具链、平台、出不出六份 JRE 必须逐次指定，而只有它能带输入
（`repository_dispatch` 没有这一层）。两者都要 PAT —— `github.token` 只作用于本仓。

tag 只能**按 run id 反查**：派发到查询之间可能有另一条自动 alpha 插进来，取"最新 tag"会
静默用到另一次构建；而 alpha/beta 的 tag 形如 `<版本名>-<通道>.<run_id>`，run id 是本仓
全局单调递增的整数。

用法（CI 里由 release.yml 的「派发伴生仓」步骤调用）：
  python3 scripts/refresh-companions.py --channel alpha --windows-toolchain all \
      --targets "windows-x64 linux-x64" --android '[{"target":"android-arm64"}]' \
      --pack-jre true --pack-oci false
输出（追加进 $GITHUB_OUTPUT）：
  ext_runtime_tag=<tag>
  tray_tag=<tag>
两者都可能为空串（本次不需要该仓）。
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

EXT_REPO = "576576/Suwayomi-ext-runtime"
TRAY_REPO = "576576/Suwayomi-tray"
WORKFLOW = "release.yml"
PLATFORMS = ("windows-x64", "windows-arm64", "linux-x64", "linux-arm64", "macos-x64", "macos-arm64")


# --------------------------------------------------------------------------- #
# HTTP
# --------------------------------------------------------------------------- #
def request(method: str, url: str, tok: str, body: dict | None = None) -> tuple[int, str]:
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("User-Agent", "suwayomi-refresh-companions")
    if tok:
        req.add_header("Authorization", f"Bearer {tok}")
    if data is not None:
        req.add_header("Content-Type", "application/json")
    # 直连：本机 HTTPS_PROXY 会间歇性失效；runner 上本来也没有代理。
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(req, timeout=60) as r:
            return r.status, r.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode("utf-8", "replace")
    except Exception as e:  # 网络层的抖动交给调用方重试
        return 0, str(e)


def api(method: str, path: str, tok: str, body: dict | None = None, tries: int = 3) -> tuple[int, str]:
    for i in range(tries):
        st, txt = request(method, API + path, tok, body)
        if st:
            return st, txt
        if i < tries - 1:
            time.sleep(5)
    return 0, txt


API = "https://api.github.com"


# --------------------------------------------------------------------------- #
# 派发参数：主仓的选择 1:1 映射到伴生仓的输入
# --------------------------------------------------------------------------- #
def jbool(v: bool) -> str:
    return "true" if v else "false"


def plain(target: str) -> str:
    """矩阵项名去掉工具链段 —— 托盘仓的平台开关没有工具链这一维。"""
    return target[: -len("-gnullvm")] if target.endswith("-gnullvm") else target


def flag_name(plat: str) -> str:
    return "build_" + plat.replace("-", "_")


def ext_inputs(channel: str, pack_jre: bool) -> dict[str, str]:
    # publish_packages 恒 false：这次派发只是为了让主仓取到新资产，不必为它堆一个
    # 不可变的 Maven 版本（版本一旦发出去就删不掉）。
    return {"channel": channel, "build_jre": jbool(pack_jre), "publish_packages": "false"}


def tray_inputs(channel: str, toolchain: str, targets: list[str]) -> dict[str, str]:
    bases = {plain(t) for t in targets}
    d = {"channel": channel, "windows_toolchain": toolchain}
    for plat in PLATFORMS:
        d[flag_name(plat)] = jbool(plat in bases)
    return d


def wanted(targets: list[str], android: list, pack_oci: bool, pack_jre: bool,
           channel: str, toolchain: str) -> list[tuple[str, str, dict]]:
    """-> [(输出键, slug, dispatch inputs)]；不需要的仓不进这个列表。

    判据对齐真正的消费者：
      ext-runtime —— 桌面每个 target 都下载沙盒 jar、OCI 镜像也要、android 要共享源码；
      tray        —— 只有桌面 target 才下载桌面壳（且一个平台都没勾时托盘仓自己会报错）。
    """
    out = []
    if targets or android or pack_oci:
        out.append(("ext_runtime_tag", EXT_REPO, ext_inputs(channel, pack_jre)))
    if targets:
        out.append(("tray_tag", TRAY_REPO, tray_inputs(channel, toolchain, targets)))
    return out


# --------------------------------------------------------------------------- #
def list_runs(slug: str, tok: str) -> list[dict]:
    st, txt = api("GET", f"/repos/{slug}/actions/workflows/{WORKFLOW}/runs"
                          f"?event=workflow_dispatch&branch=main&per_page=50", tok)
    if st != 200:
        raise RuntimeError(f"{slug} 拉 run 列表失败（HTTP {st}）：{txt[:200]}")
    return json.loads(txt).get("workflow_runs", [])


def run_state(slug: str, run_id: str, tok: str) -> dict:
    st, txt = api("GET", f"/repos/{slug}/actions/runs/{run_id}", tok)
    if st != 200:
        raise RuntimeError(f"{slug} 取 run {run_id} 失败（HTTP {st}）：{txt[:200]}")
    return json.loads(txt)


def tag_for_run(slug: str, run_id: str, channel: str, tok: str) -> str:
    """反查这次 run 发出来的 tag —— alpha/beta 的 tag 尾段就是 `-<channel>.<run_id>`。"""
    suffix = f"-{channel}.{run_id}"
    st, txt = api("GET", f"/repos/{slug}/releases?per_page=30", tok)
    if st == 200:
        for r in json.loads(txt) or []:
            tag = (r.get("tag_name") or "").strip()
            if tag.endswith(suffix):
                return tag
    return ""


def refresh(label: str, slug: str, inputs: dict, tok: str, channel: str,
            timeout: int, interval: int, dry_run: bool) -> str:
    print(f"== {label}：{slug} ==")
    print(f"   dispatch inputs: {json.dumps(inputs, ensure_ascii=False, sort_keys=True)}")
    if dry_run:
        return ""

    before = {str(r["id"]) for r in list_runs(slug, tok)}
    print(f"   派发前的 workflow_dispatch run 数：{len(before)}")

    st, txt = api("POST", f"/repos/{slug}/actions/workflows/{WORKFLOW}/dispatches", tok,
                  {"ref": "main", "inputs": inputs})
    if st not in (204, 201, 200):
        raise RuntimeError(f"{slug} 派发失败（HTTP {st}）：{txt[:300]}")
    print("   已派发")

    deadline = time.time() + timeout

    # 1) 等新 run 出现。用"派发前的 id 集合"作差，不靠时间戳 —— 时钟偏移会让窗口判断出错。
    run = None
    while time.time() < deadline:
        new = [r for r in list_runs(slug, tok) if str(r["id"]) not in before]
        if new:
            if len(new) > 1:
                print(f"::warning::派发后同时出现 {len(new)} 条新 run，取 id 最大的那条")
            run = max(new, key=lambda r: int(r["id"]))
            break
        time.sleep(interval)
    if run is None:
        raise RuntimeError(f"{slug} 派发后 {timeout} 秒内没看到新的 workflow_dispatch run")
    run_id = str(run["id"])
    print(f"   新 run：{run_id}（{run.get('html_url', '')}）")

    # 2) 等它跑完
    while time.time() < deadline:
        cur = run_state(slug, run_id, tok)
        if cur.get("status") == "completed":
            break
        time.sleep(interval)
    else:
        raise RuntimeError(f"{slug} 的 run {run_id} 在 {timeout} 秒内没跑完")

    if cur.get("conclusion") != "success":
        failed = []
        st, txt = api("GET", f"/repos/{slug}/actions/runs/{run_id}/jobs?per_page=100", tok)
        if st == 200:
            failed = [j["name"] for j in json.loads(txt).get("jobs", [])
                      if j.get("conclusion") not in ("success", "skipped", None)]
        raise RuntimeError(f"{slug} 的 run {run_id} 结论是 {cur.get('conclusion')}"
                           + (f"，失败 job：{failed}" if failed else ""))

    # 3) 反查 tag（release 由 run 里的 publish job 创建，偶尔要等几秒才查得到）
    for _ in range(10):
        tag = tag_for_run(slug, run_id, channel, tok)
        if tag:
            print(f"   tag：{tag}")
            return tag
        time.sleep(interval)
    raise RuntimeError(f"{slug} 的 run {run_id} 跑完了，但没找到以 '{channel}.{run_id}' "
                       f"结尾的 release tag")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--channel", required=True, choices=["alpha", "beta"])
    ap.add_argument("--windows-toolchain", default="msvc")
    ap.add_argument("--targets", default="", help="主仓选中的桌面 target（空格分隔）")
    ap.add_argument("--android", default="[]")
    ap.add_argument("--pack-jre", default="false")
    ap.add_argument("--pack-oci", default="false")
    ap.add_argument("--api-base", default=None, help="仅测试用：把 API 指到一个假服务")
    ap.add_argument("--timeout", type=int, default=3600)
    ap.add_argument("--interval", type=int, default=20)
    ap.add_argument("--dry-run", action="store_true")
    a = ap.parse_args()

    global API
    API = (a.api_base or API).rstrip("/")

    targets = a.targets.split()
    try:
        android = json.loads(a.android or "[]")
    except Exception:
        android = []
    pack_jre = a.pack_jre == "true"
    pack_oci = a.pack_oci == "true"

    tok = os.environ.get("COMPANION_DISPATCH_TOKEN") or ""
    if not tok and not a.dry_run:
        print("::error::没有 COMPANION_DISPATCH_TOKEN —— 跨仓派发不能用 github.token"
              "（它只能操作本仓）", file=sys.stderr)
        return 1

    todos = wanted(targets, android, pack_oci, pack_jre, a.channel, a.windows_toolchain)
    if not todos:
        print("本次没有需要刷新的伴生仓，跳过")
        return 0

    if a.dry_run:
        for label, slug, inp in todos:
            refresh(label, slug, inp, tok, a.channel, a.timeout, a.interval, True)
        return 0

    out = {}
    for label, slug, inp in todos:
        out[label] = refresh(label, slug, inp, tok, a.channel, a.timeout, a.interval, False)
    for k in ("ext_runtime_tag", "tray_tag"):
        print(f"{k}={out.get(k, '')}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except RuntimeError as e:
        print(f"::error::{e}", file=sys.stderr)
        sys.exit(1)
