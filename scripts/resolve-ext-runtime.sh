#!/usr/bin/env bash
# Resolve the ext-runtime.jar release-asset URL published by Suwayomi-ext-runtime.
#
# Usage: bash scripts/resolve-ext-runtime.sh [--sources] [--stable|--build] [<version>]
#   （无参数） 取最新符合条件的 release 资产
#   --sources  取共享源码包 ext-runtime-<V>-shared-sources.jar（Android 用）
#   --stable   只看非预发布 release（Suwayomi-next 正式发布通道用）
#   --build    只看预发布 release（ext-runtime 推 main 自动出的那批 alpha）
#   <version>  取指定版本，如 30.0.47（对应 tag v30.0.47；大版本是 AOSP API level）
#
# 不加 --stable / --build 时不区分预发布 —— ext-runtime 现在推 main 就会自动出
# alpha，区分不开会把自动构建的版本混进正式发布包（与 scripts/resolve-webui.sh 同套做法）。
#
# 输出：把三个值写到 stdout，格式为
#   url=<下载地址>
#   version=<版本号>
#   base=<该 release 的资产下载前缀>   # 裁剪 JRE 按 <base>/ext-runtime-jre-<V>-<os>-<arch>.tar.gz 取
# 失败时打 ::error:: 并退出 1（由 CI 中止）。
#
# 为什么走 Release 资产而不是 GitHub Packages：
#   桌面 target 只是把这个 jar 拷进 bin/，不需要 Maven 坐标；Android 侧要的是**源码**
#   （Kotlin 元数据版本对不上，编译产物没法消费）。两条链都吃 Release 资产，Release
#   资产**免鉴权**，省掉一个 PAT secret，也没有 PAT 过期导致 401 的风险。
#
# 判据：资产名以 .jar 结尾，且**不以 -shared-sources.jar 结尾** ——
#   后者是共享源码包，不能被当成沙盒 jar 塞进 bin/。
#
# 三级探测，逐级兜底（与 scripts/resolve-webui.sh 同构）：
#   1. gh api    —— runner 预装；workflow 注入 GH_TOKEN 后无限流、跨仓库也不会 404
#   2. 匿名 REST API
#   3. 匿名 HTML —— releases.atom + expanded_assets 页

set -uo pipefail

WANT=""
KIND="jar"   # jar = 桌面沙盒 jar；sources = 共享源码包
PICK="any"   # any / stable / build ——是否按 prerelease 过滤
for arg in "$@"; do
  case "$arg" in
    --sources) KIND="sources" ;;
    --stable)  PICK="stable" ;;
    --build)   PICK="build"  ;;
    v*) echo "::error::resolve-ext-runtime.sh: 版本号不要带 v 前缀，收到 '${arg}'" >&2; exit 1 ;;
    *)  WANT="$arg" ;;
  esac
done
REPO="576576/Suwayomi-ext-runtime"
UA='Mozilla/5.0 (Windows NT 10.0; Win64; x64)'

# 从 releases JSON 里挑出 ext-runtime-<V>.jar：draft 一律跳过；
# 指定版本时只认 tag v<V>，否则按 created_at 取最新。
PY_PICK='
import json, re, sys

want, kind, pick = sys.argv[1], sys.argv[2], sys.argv[3]
want_pre = None if pick == "any" else (pick == "build")
pat = re.compile(r"^ext-runtime-(.+)-shared-sources\.jar$" if kind == "sources"
                 else r"^ext-runtime-(.+)\.jar$")
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
        # 两种资产同名前缀、只差后缀，必须按 kind 严格区分，不能只做 endswith 排除
        if (name.endswith("-shared-sources.jar")) != (kind == "sources"):
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

pick() { printf '%s' "$1" | python3 -c "$PY_PICK" "$WANT" "$KIND" "$PICK" 2>/dev/null || true; }

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

# 3) 匿名 HTML：atom 拿最近 tag → expanded_assets 页抽 .jar
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
    if [ "$KIND" = "sources" ]; then
      ASSET_GREP="grep    'shared-sources'"
    else
      ASSET_GREP="grep -v 'shared-sources'"
    fi
    URL="$(printf '%s' "$PAGE" | grep -oE "/${REPO}/releases/download/[^\"]*ext-runtime-[^\"/]*\.jar" \
      | eval "$ASSET_GREP" | head -1 | sed 's|^|https://github.com|')" || true
    if [ -n "$URL" ]; then
      OUT="$URL
$(printf '%s' "$URL" | sed -E 's|.*/ext-runtime-(.*)\.jar$|\1|; s|-shared-sources$||')"
      break
    fi
  done
fi

if [ -z "$OUT" ]; then
  echo "::error::无法解析 ${REPO} 的 ext-runtime jar 资产（version=${WANT:-latest}：gh / API / HTML 三级探测均无结果）" >&2
  exit 1
fi

URL="$(printf '%s' "$OUT" | sed -n '1p')"
VER="$(printf '%s' "$OUT" | sed -n '2p')"

if [ -z "$URL" ] || [ -z "$VER" ]; then
  echo "::error::解析结果不完整：url='${URL}' version='${VER}'" >&2
  exit 1
fi

# base = 这个 release 的资产下载前缀。同一版本下的其余资产（裁剪 JRE）按
#   <base>/ext-runtime-jre-<V>-<os>-<arch>.tar.gz
# 拼出来即可，不用为每种 (os, arch) 再探测一遍 —— 它们在同一个 release 里。
BASE="${URL%/*}"

echo "url=$URL"
echo "version=$VER"
echo "base=$BASE"
