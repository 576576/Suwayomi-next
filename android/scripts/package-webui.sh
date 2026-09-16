#!/usr/bin/env bash
# 把一份 WebUI 构建产物打成 android/app/src/main/assets/webui.zip。
#
# 用法：
#   android/scripts/package-webui.sh <webui 目录 或 .zip>
#   android/scripts/package-webui.sh                # 自动找 ../Suwayomi-WebUI/build
#
# 为什么单独打包成 assets 里的 zip 而不是直接放进 assets/ 目录：
#  * assets 里的文件是逐条目打开的，几十个文件会让 AssetManager 索引很大；
#  * zip 一次解压到 filesDir，之后由 server 直接以静态文件提供，
#    与桌面「WebUI 目录」的形态一致（server 只认目录 + version.txt）。
#
# 硬性要求：包根必须有 version.txt —— 它是 server 判定「本地 WebUI 版本」的
# 唯一来源（aboutWebUI.tag / channel / build_time 都读它）。
#
# 打包用 python3 而不是 `zip` 命令：Git Bash 环境不预装 zip，而 python3
# 在本地与所有 CI runner 上都有。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="$REPO_ROOT/android/app/src/main/assets/webui.zip"

PY="$(command -v python3 || command -v python)"

SRC="${1:-}"
if [[ -z "$SRC" ]]; then
  for cand in "${SUWAYOMI_WEBUI_DIR:-}" "$REPO_ROOT/../Suwayomi-WebUI/build"; do
    if [[ -n "$cand" && -d "$cand" ]]; then SRC="$cand"; break; fi
  done
fi

if [[ -z "$SRC" || ! -e "$SRC" ]]; then
  echo "错误：找不到 WebUI 构建产物。" >&2
  echo "  用法：android/scripts/package-webui.sh <dir|zip>" >&2
  exit 1
fi

mkdir -p "$(dirname "$DEST")"

# Git Bash 给的是 /e/Github/... 形式的 MSYS 路径，Windows 版 python 认不出，
# 传给原生程序前先转成 C:\... 形式（Linux/macOS 上 cygpath 不存在，跳过）。
if command -v cygpath >/dev/null 2>&1; then
  SRC="$(cygpath -w "$SRC")"
  DEST="$(cygpath -w "$DEST")"
fi

"$PY" - "$SRC" "$DEST" <<'PY'
import sys, zipfile, pathlib

src, dest = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])

if src.is_dir():
    files = sorted(p for p in src.rglob("*") if p.is_file())
    if not any(p.relative_to(src).as_posix() == "version.txt" for p in files):
        sys.exit(
            "错误：%s 下没有 version.txt。\n"
            "  WebUI 的 vite build 不产出它，需要先跑：\n"
            "    npx tsx tools/scripts/writeVersionFile.ts" % src
        )
    dest.unlink(missing_ok=True)
    # 用 ZIP_DEFLATED 压：JS/CSS 能压到 1/3 左右，解压时也仍是标准 zip
    with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for p in files:
            z.write(p, p.relative_to(src).as_posix())
else:
    with zipfile.ZipFile(src) as z:
        names = [n for n in z.namelist() if n.rstrip("/") == "version.txt" or n.endswith("/version.txt")]
        if not names:
            sys.exit("错误：%s 里没有 version.txt" % src)
    dest.write_bytes(src.read_bytes())

with zipfile.ZipFile(dest) as z:
    version = z.read("version.txt").decode().strip()
print("已生成 %s (%.1f MB)" % (dest, dest.stat().st_size / 1024 / 1024))
for line in version.splitlines():
    print("  version.txt: %s" % line)
PY
