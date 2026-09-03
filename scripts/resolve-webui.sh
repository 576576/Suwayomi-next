#!/usr/bin/env bash
# Resolve the .zip download URL of a Suwayomi-WebUI release asset.
#
# Usage: bash scripts/resolve-webui.sh <build|stable>
#   build  — 最新构建：WebUI CI 每次推送产出的 r{commitCount}（prerelease）
#   stable — 最新正式 release：3.y.z（非 prerelease、非 draft）
#
# 通道策略由调用方决定：alpha / beta → build（跟最新构建），release → stable（跟最新
# 正式版）。本脚本只负责“按类型挑出最新一个 .zip”，不知道调用方是什么通道。
#
# 输出：仅把 browser_download_url 打到 stdout；失败时打 ::error:: 并退出 1（由 CI 中止）。
#
# 三级探测，逐级兜底、失败静默：
#   1. gh api    —— runner 预装；workflow 注入 GH_TOKEN 后无限流、跨仓库也不会 404
#   2. 匿名 REST API
#   3. 匿名 HTML —— releases.atom + release 页；无法区分通道，取最新 tag 上的第一个
#                   .zip，仅用于兜底（宁可类型不对，也不要让打包直接失败）

set -uo pipefail

KIND="${1:-}"
case "$KIND" in
  build|stable) ;;
  *)
    echo "::error::resolve-webui.sh: 参数应为 build|stable，收到 '${KIND}'" >&2
    exit 1
    ;;
esac

REPO="576576/Suwayomi-WebUI"
UA='Mozilla/5.0 (Windows NT 10.0; Win64; x64)'

# 从 releases JSON 中挑出目标类型的 .zip：draft 一律跳过，prerelease 与 KIND 对应，
# 再按 created_at 取最新（API 已是倒序，显式排序防顺序变化）。
PY_PICK='
import json, sys

want_pre = sys.argv[1] == "build"
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
    if bool(r.get("prerelease")) != want_pre:
        continue
    for a in r.get("assets") or []:
        a = a or {}
        if (a.get("name") or "").endswith(".zip") and a.get("browser_download_url"):
            cands.append((r.get("created_at") or "", a["browser_download_url"]))

if not cands:
    sys.exit(0)
cands.sort(key=lambda c: c[0], reverse=True)
print(cands[0][1])
'

pick() { printf '%s' "$1" | python3 -c "$PY_PICK" "$KIND" 2>/dev/null || true; }

ZIP=""

# 1) gh api
if [ -z "$ZIP" ] && command -v gh >/dev/null 2>&1; then
  RAW="$(gh api "repos/${REPO}/releases?per_page=50" 2>/dev/null || true)"
  if [ -n "$RAW" ]; then ZIP="$(pick "$RAW")"; fi
fi

# 2) 匿名 REST API
if [ -z "$ZIP" ]; then
  RAW="$(curl -sL --retry 2 --max-time 30 -A "$UA" \
    "https://api.github.com/repos/${REPO}/releases?per_page=50" 2>/dev/null || true)"
  if [ -n "$RAW" ]; then ZIP="$(pick "$RAW")"; fi
fi

# 3) 匿名 HTML：atom 拿最近 tag → 每个 tag 先 expanded_assets 再 release 页抽 .zip
if [ -z "$ZIP" ]; then
  TAGS="$(curl -sL --max-time 30 -A "$UA" "https://github.com/${REPO}/releases.atom" \
    | python3 -c "import re,sys,html; h=sys.stdin.read(); ts=[html.unescape(t).strip() for t in re.findall(r'releases/tag/([^/\"><]+)', h)]; u=[]; [u.append(t) for t in ts if t not in u]; print('\n'.join(u[:10]))" 2>/dev/null || true)"
  for T in $TAGS; do
    PAGE="$(curl -sL --max-time 30 -A "$UA" "https://github.com/${REPO}/releases/expanded_assets/${T}" 2>/dev/null || true)"
    ZIP="$(printf '%s' "$PAGE" | grep -oE "/${REPO}/releases/download/[^\"]*\.zip" | head -1 | sed 's|^|https://github.com|')" || true
    if [ -z "$ZIP" ]; then
      PAGE="$(curl -sL --max-time 30 -A "$UA" "https://github.com/${REPO}/releases/tag/${T}" 2>/dev/null || true)"
      ZIP="$(printf '%s' "$PAGE" | grep -oE "/${REPO}/releases/download/[^\"]*\.zip" | head -1 | sed 's|^|https://github.com|')" || true
    fi
    if [ -n "$ZIP" ]; then break; fi
  done
fi

if [ -z "$ZIP" ]; then
  echo "::error::无法解析 ${REPO} 的 .zip 资产（kind=${KIND}：gh / API / HTML 三级探测均无结果）" >&2
  exit 1
fi

printf '%s\n' "$ZIP"
