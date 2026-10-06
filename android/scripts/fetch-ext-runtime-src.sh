#!/usr/bin/env bash
# 取 ext-runtime 的**共享源码包**，展开成 :extension-host 的源目录。
#
# 用法：
#   android/scripts/fetch-ext-runtime-src.sh [<tag>]
#
#   <tag> 是 ext-runtime 那个 release 的 tag（`v30.0.47`，alpha 形如
#   `36.0.67-alpha.<run_id>`），不带则取最新预发布。**别传裸版本号**：alpha 的
#   tag 带 `-alpha.<run_id>` 段、同一个版本号会被多次构建复用，只给版本号匹配不到，
#   而不给 tag 时解析脚本会退化成"最新预发布"—— 那可能与主仓那批 jar 不是同一次构建。
#
# 为什么是源码而不是编译好的 jar：
#   :extension-host 用 AGP 内置的 Kotlin 2.3.20，ext-runtime 用 Kotlin 2.4.0。
#   2.4 编出来的 class 元数据版本 2.3 读不了（编译期就报 "was compiled with an
#   incompatible version of Kotlin"），所以只能给源码、让 Android 侧自己编译。
#
# 为什么走 Release 资产而不是 GitHub Packages：
#   二者发布的都是同一份 ext-runtime-<V>-shared-sources.jar，但 Packages **必须鉴权**
#   （公开包也要 token），Release 资产免鉴权。反正都要下载再展开（Gradle 没法直接把
#   一个依赖当源目录），用免鉴权的那条通道就省掉一个 PAT secret 和它的过期风险。
#
# 落到 build/ 而不是 src/：这是一份**下载来的**输入，不该进版本库，
# 也不该和本仓库自己写的源码混在一个目录里。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TAG="${1:-}"
DEST="$REPO_ROOT/android/build/ext-runtime-src"

PY="$(command -v python3 || command -v python)"

OUT="$(bash "$REPO_ROOT/scripts/resolve-ext-runtime.sh" --sources ${TAG:+"$TAG"})"
URL="$(printf '%s' "$OUT" | sed -n 's/^url=//p')"
GOT="$(printf '%s' "$OUT" | sed -n 's/^version=//p')"
BASE="$(printf '%s' "$OUT" | sed -n 's/^base=//p')"

if [ -z "$URL" ] || [ -z "$GOT" ] || [ -z "$BASE" ]; then
  echo "::error::fetch-ext-runtime-src.sh: 解析结果不完整：url='${URL}' version='${GOT}' base='${BASE}'" >&2
  exit 1
fi
if [ -n "$TAG" ]; then
  # 实得的 release 必须就是请求的那个：资产名里的版本号（36.0.67）会被多次 alpha 构建复用，
  # 拿它做判据挡不住"取到了另一次构建的源码"。
  GOT_TAG="${BASE##*/}"; GOT_TAG="${GOT_TAG#v}"
  WANT_TAG="${TAG#v}"
  if [ "$GOT_TAG" != "$WANT_TAG" ]; then
    echo "::error::fetch-ext-runtime-src.sh: release 不符，请求 '${TAG}' 实得 '${BASE##*/}'" >&2
    exit 1
  fi
fi

echo "ext-runtime 共享源码：$GOT（${BASE##*/}）"
echo "  $URL"

rm -rf "$DEST"
mkdir -p "$(dirname "$DEST")"
curl -fsSL --retry 3 --max-time 120 -o "$DEST.jar" "$URL"

# 用 python 解开：Git Bash / macOS 都不保证有 unzip
"$PY" "$REPO_ROOT/scripts/unzip_any.py" "$DEST.jar" "$DEST"

# 展开了不代表能编译：这三个包根是 :extension-host 的全部输入，缺任何一个
# 都会在 Gradle 配置期之后才炸，且报错信息指向的是"找不到符号"而不是"下载错了"。
for d in eu/kanade/tachiyomi suwayomi/tachidesk sandbox; do
  if [ ! -d "$DEST/$d" ]; then
    echo "::error::共享源码包缺少 $d/（version=$GOT）—— 展开内容：$DEST" >&2
    ls -la "$DEST" >&2 || true
    exit 1
  fi
done

N="$(find "$DEST" -name '*.kt' | wc -l | tr -d ' ')"
if [ "$N" -lt 10 ]; then
  echo "::error::共享源码包只展开出 $N 个 .kt，明显不完整（version=$GOT）" >&2
  exit 1
fi

echo "已展开到 ${DEST}（${N} 个 .kt）"
