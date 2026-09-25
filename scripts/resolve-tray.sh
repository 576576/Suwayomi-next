#!/usr/bin/env bash
# Resolve the desktop-shell (tray) binary published by Suwayomi-tray.
#
# Usage: bash scripts/resolve-tray.sh [--optional] [--stable|--build] [<version>]
#   （无参数） 取最新符合条件的 release 里的桌面壳资产
#   --optional 解析不到时只打 ::warning:: 并以 0 退出（不给就是 ::error:: + exit 1）
#   --stable   只看非预发布 release（Suwayomi-next 正式发布通道用）
#   --build    只看预发布 release（alpha / beta 自动构建出的那批）
#   <version>  取指定版本，如 1.0.23（对应 tag v1.0.23）
#
# 不加 --stable / --build 时不区分预发布 —— 桌面壳现在推 main 就会自动出 alpha，
# 区分不开会把自动构建的版本混进正式发布包（与 scripts/resolve-webui.sh 同套做法）。
#
# 输出：把三个值写到 stdout，格式为
#   url=<任意一个桌面壳资产的下载地址>
#   version=<版本号>
#   base=<该 release 的资产下载前缀>
# 调用方按 <base>/suwayomi-tray-<V>-<target>[.exe] 取自己那一份 —— 六个桌面 target 的
# 资产都在同一个 release 里，不必每个 target 探测一遍（与裁剪 JRE 同一套做法）。
#
# 桌面壳是独立仓库发布的：托盘代码没变就不重编，主仓库每次构建只下载几 MB。
# 所以这里解析失败**不应该**让发布失败 —— 打 ::warning::，包里没有托盘壳而已。
#
# 三级探测，逐级兜底（与 scripts/resolve-webui.sh 同构）：
#   1. gh api    —— runner 预装；workflow 注入 GH_TOKEN 后无限流、跨仓库也不会 404
#   2. 匿名 REST API
#   3. 匿名 HTML —— releases.atom + expanded_assets 页

set -uo pipefail

WANT=""
OPTIONAL="no"
PICK="any"   # any / stable / build ——是否按 prerelease 过滤
for arg in "$@"; do
  case "$arg" in
    --optional) OPTIONAL="yes" ;;
    --stable)   PICK="stable" ;;
    --build)    PICK="build"  ;;
    v*) echo "::error::resolve-tray.sh: 版本号不要带 v 前缀，收到 '${arg}'" >&2; exit 1 ;;
    *)  WANT="$arg" ;;
  esac
done
REPO="576576/Suwayomi-tray"
UA='Mozilla/5.0 (Windows NT 10.0; Win64; x64)'

# 从 releases JSON 里挑出 suwayomi-tray-<V>-<target>[.exe]：draft 一律跳过；
# 指定版本时只认 tag v<V>，否则按 created_at 取最新。
PY_PICK='
import json, re, sys

want, pick = sys.argv[1], sys.argv[2]
want_pre = None if pick == "any" else (pick == "build")
pat = re.compile(r"^suwayomi-tray-(\d+\.\d+\.\d+)-([A-Za-z0-9-]+?)(\.exe)?$")
try:
    rels = json.load(sys.stdin)
except Exception:
    sys.exit(0)
if not isinstance(rels, list):
    sys.exit(0)

cands = []
for r in rels:
    if not isinstance(r, dict) or r.get("draft"):
        continue
    if want_pre is not None and bool(r.get("prerelease")) != want_pre:
        continue
    tag = (r.get("tag_name") or "").lstrip("v")
    if want and tag != want:
        continue
    for a in r.get("assets") or []:
        a = a or {}
        name = a.get("name") or ""
        url = a.get("browser_download_url")
        if not url:
            continue
        m = pat.match(name)
        if not m:
            continue
        # 资产名里的版本号优先；退回 tag
        cands.append((r.get("created_at") or "", url, m.group(1) or tag))

if not cands:
    sys.exit(0)
cands.sort(key=lambda c: c[0], reverse=True)
_, url, ver = cands[0]
print(url)
print(ver)
'

pick() { printf '%s' "$1" | python3 -c "$PY_PICK" "$WANT" "$PICK" 2>/dev/null || true; }

OUT=""

# 1) gh api
if [ -z "$OUT" ] && command -v gh >/dev/null 2>&1; then
  RAW="$(gh api "repos/${REPO}/releases?per_page=50" 2>/dev/null || true)"
  if [ -n "$RAW" ]; then OUT="$(pick "$RAW")"; fi
fi

# 2) 匿名 REST API
if [ -z "$OUT" ]; then
  RAW="$(curl -sL --retry 2 --max-time 30 -A "$UA" \
    "https://api.github.com/repos/${REPO}/releases?per_page=50" 2>/dev/null || true)"
  if [ -n "$RAW" ]; then OUT="$(pick "$RAW")"; fi
fi

# 3) 匿名 HTML：atom 拿最近 tag → expanded_assets 页抽桌面壳资产
#    HTML 页区分不出 prerelease，这一级只保证"不失败"，可能与 --stable/--build 不符；
#    正常环境前两级（gh / REST）一定命中，走不到这里。
if [ -z "$OUT" ]; then
  if [ -n "$WANT" ]; then
    TAGS="v${WANT}"
  else
    TAGS="$(curl -sL --max-time 30 -A "$UA" "https://github.com/${REPO}/releases.atom" \
      | python3 -c "import re,sys,html; h=sys.stdin.read(); ts=[html.unescape(t).strip() for t in re.findall(r'releases/tag/([^/\"><]+)', h)]; u=[]; [u.append(t) for t in ts if t not in u]; print('\n'.join(u[:10]))" 2>/dev/null || true)"
  fi
  for T in $TAGS; do
    PAGE="$(curl -sL --max-time 30 -A "$UA" "https://github.com/${REPO}/releases/expanded_assets/${T}" 2>/dev/null || true)"
    URL="$(printf '%s' "$PAGE" | grep -oE "/${REPO}/releases/download/[^\"]*suwayomi-tray-[^\"/]*" \
      | head -1 | sed 's|^|https://github.com|')" || true
    if [ -n "$URL" ]; then
      OUT="$URL
$(printf '%s' "$URL" | sed -E 's|.*/suwayomi-tray-([0-9]+\.[0-9]+\.[0-9]+)-.*|\1|')"
      break
    fi
  done
fi

if [ -z "$OUT" ]; then
  MSG="无法解析 ${REPO} 的桌面壳资产（version=${WANT:-latest}：gh / API / HTML 三级探测均无结果）"
  if [ "$OPTIONAL" = "yes" ]; then
    echo "::warning::${MSG}"
    exit 0
  fi
  echo "::error::${MSG}" >&2
  exit 1
fi

URL="$(printf '%s' "$OUT" | sed -n '1p')"
VER="$(printf '%s' "$OUT" | sed -n '2p')"

if [ -z "$URL" ] || [ -z "$VER" ]; then
  MSG="解析结果不完整：url='${URL}' version='${VER}'"
  if [ "$OPTIONAL" = "yes" ]; then
    echo "::warning::${MSG}"
    exit 0
  fi
  echo "::error::${MSG}" >&2
  exit 1
fi

# base = 这个 release 的资产下载前缀。同版本下其余 target 的资产按
#   <base>/suwayomi-tray-<V>-<target>[.exe]
# 拼出来即可，不用为每个 target 再探测一遍 —— 它们在同一个 release 里。
BASE="${URL%/*}"

echo "url=$URL"
echo "version=$VER"
echo "base=$BASE"
