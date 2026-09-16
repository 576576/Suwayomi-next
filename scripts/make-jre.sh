#!/usr/bin/env bash
# 用 jlink 生成**裁剪后的** Java 运行时，供 `+jre` 产物使用。
#
# 用法：
#   scripts/make-jre.sh <windows|linux|mac> <x64|aarch64> <输出目录>
#
# 例：scripts/make-jre.sh windows x64 dist/Suwayomi-3.20.5-release-windows-x64+jre/jre
#
# 三件事决定了这个脚本长这样：
#
# 1. **jmods 从哪来。** Temurin JDK 24 起启用 JEP 493，JDK 归档里不再带
#    `jmods/` 目录（本机 jdk-25.0.4.7 就没有），而 jlink 的 `--module-path`
#    正需要它。所以优先看宿主 `$JAVA_HOME/jmods/` 有没有（有就直接用），
#    没有才去 Adoptium 下那个单独的 jmods 包（约 85MB）。
#    需要"宿主自带"这条兜底是因为 **Adoptium 对 `windows/aarch64` 根本没发
#    JDK 25 的任何制品**（jdk / jre / jmods 全 404，该平台只到 JDK 21），
#    那个 target 只能换 Azul Zulu 装 JDK（归档自带 jmods/）。
#    ⚠ 判断「有没有」时**不能拿 `$JAVA_HOME` 直接做路径名展开**：CI 注入的是
#    Windows 形式（`C:\hostedtoolcache\…`），反斜杠会被 glob 当转义符吃掉，
#    于是 Zulu 自带的 70 个 .jmod 被判成"没有"（run 35078586922）。见 `jmods_dir`。
# 2. **jlink 不能跨平台生成镜像。** 实测：Windows 的 jlink + linux-aarch64 的
#    jmods，产出的 `bin/java` 是 PE 头（`MZ`）加一堆 `.dll` —— jlink 的 launcher
#    与原生库取自**宿主** JDK，不取自 `--module-path`。所以下面先校验宿主平台与
#    目标平台一致，不一致直接报错退出：宁可让 CI 明确失败，也不要产出一个
#    "看着打包成功、装上就 UnsatisfiedLinkError" 的运行时。
#    （CI 因此给每个平台分配**原生** runner，见 build.yml 的 target mapping。）
#    两道闸：入口处比「宿主 vs 目标」，末尾处核「产物 magic + 架构」
#    （`assert_native_artifact`，被 .workbuddy/verify/check_jre_arch.sh 单测）。
#    **宿主架构必须问 JDK 二进制，不能问 `uname -m`** —— Windows ARM64 runner 上的
#    Git Bash 是 x64 版，`uname -m` 报 x86_64。详见下面 `binary-probe` 区块的头注释。
# 3. **模块白名单是实测出来的**，不是抄的，见 MODULES 处的注释。
#
# 裁剪效果（Windows x64 实测）：完整 Temurin JRE 25 解压 180MB / 压缩 58MB；
# 本脚本产出解压 39MB / 压缩 25MB —— 解压 −78%，压缩 −57%。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

TARGET_OS="${1:-}"
TARGET_ARCH="${2:-}"
OUT="${3:-}"

if [[ -z "$TARGET_OS" || -z "$TARGET_ARCH" || -z "$OUT" ]]; then
  echo "用法：scripts/make-jre.sh <windows|linux|mac> <x64|aarch64> <输出目录>" >&2
  exit 2
fi

case "$TARGET_OS" in
  windows|linux|mac) ;;
  *) echo "错误：os 应为 windows|linux|mac，收到 '$TARGET_OS'" >&2; exit 2 ;;
esac
case "$TARGET_ARCH" in
  x64|aarch64) ;;
  *) echo "错误：arch 应为 x64|aarch64，收到 '$TARGET_ARCH'" >&2; exit 2 ;;
esac

# >>> binary-probe >>>
# 下面这组函数被 .workbuddy/verify/check_jre_arch.sh 按标记**整块抽出去**单测，
# 所以标记内必须是能独立 source 的纯函数（不读 TARGET_*、不赋全局变量），
# 标记外也别往里塞调用。
#
# 这一组只做一件事：**问宿主 JDK「你是什么、能给 jlink 什么」**。
# 两个用途共用同一张偏移表：
#
#   1. `host_arch` —— 判断宿主 JDK 的架构，用来在入口处挡「jlink 跨平台」。
#      不能问 shell：`uname -m` 只有在「shell 自己跑在原生架构上」时才等于宿主 CPU
#      架构，而这个前提在 Windows ARM64 runner 上不成立 —— 镜像是原生 arm64、装的
#      也是货真价实的 `win_aarch64` JDK（zulu25.36.15-ca-jdk25.0.4-win_aarch64），
#      但 runner 上的 **Git for Windows 是 x64 版**，MSYS 的 `uname -m` 于是一路报
#      `x86_64`。**windows-arm64 首次真跑就死在这条自检上**（run 35074440661），
#      jlink 根本没来得及启动。真正决定 jlink 产出平台的是它拿来链接的那个 JDK
#      （launcher 与原生库取自 `$JAVA_HOME` 自身的 jmods，不取自 `--module-path`），
#      所以直接读 `$JAVA_HOME/bin/java` 的头 —— 与 shell 的仿真状态、发行版的字段
#      拼写都无关。读不到才退到 `release` 的 OS_ARCH，再退到 `uname -m`。
#   2. `assert_native_artifact` —— 校验 jlink 产物确实是目标平台（见该函数注释）。
#   3. `jmods_dir` —— 宿主 JDK 自带的 jmods 目录在哪（有就省掉一次 85MB 下载）。
#      与上面共享的其实是同一条经验：**Windows 形式路径不能直接喂给 bash 的
#      路径名展开**（详见该函数注释）。
#
# 各格式「架构」字段的偏移（都是小端）：
#
#    Windows  PE      magic 4d 5a → e_lfanew@0x3c(u32) → machine@e_lfanew+4(u16)
#                                   0x8664 = 34404  = x64
#                                   0xaa64 = 43620  = arm64
#    Linux    ELF     magic 7f 45 4c 46 → e_machine@0x12(u16)
#                                   62  = EM_X86_64 / 183 = EM_AARCH64
#    macOS    Mach-O  magic cf fa ed fe（64 位小端）→ cputype@4(u32)
#                                   0x01000007 = 16777223 = x86_64
#                                   0x0100000c = 16777228 = arm64
#             Mach-O  magic ca fe ba be（通用二进制）→ 无单一 cputype

# 读 little-endian 整数（od 只按字节吐十进制，自己按位拼回去）。
read_le() { # <文件> <偏移> <字节数>
  od -An -tu1 -j "$2" -N "$3" "$1" | tr -s ' \n' '\n' | grep -v '^$' \
    | awk -v n="$3" '{a[NR]=$1} END{v=0; for(i=n;i>=1;i--) v=v*256+a[i]; print v}'
}

binary_magic() { # <文件> -> 8 位十六进制
  head -c 4 "$1" | od -An -tx1 | tr -d ' \n'
}

# 格式分类。**三种都要认**：早期版本只区分 PE 与 ELF，macOS 被并进 else 分支按 ELF
# 判 —— 结果 macos-arm64 明明 jlink 成功产出了正确的 Mach-O，却在这里报
# "不是 ELF（平台判定错了？）" 退出 1（run 35064160508）。
binary_format() { # <文件> -> pe|elf|macho|macho-fat|unknown
  case "$(binary_magic "$1")" in
    4d5a*)            echo pe ;;
    7f454c46)         echo elf ;;
    cffaedfe|cefaedfe) echo macho ;;
    cafebabe)         echo macho-fat ;;
    *)                echo unknown ;;
  esac
}

# 架构（**格式已知**才有意义；fat 二进制返回 unknown）。
# 与 `binary_format` 拆开、共用偏移表，是为了让「宿主探测」和「产物校验」读到
# 完全相同的判据 —— 两处各写一份 0x8664 迟早会漂移。
binary_arch() { # <文件> -> x64|aarch64|unknown
  local bin="$1" m
  case "$(binary_format "$bin")" in
    pe)    m="$(read_le "$bin" "$(( $(read_le "$bin" 60 4) + 4 ))" 2)"
           case "$m" in 34404) echo x64 ;; 43620) echo aarch64 ;; *) echo unknown ;; esac ;;
    elf)   m="$(read_le "$bin" 18 2)"
           case "$m" in 62) echo x64 ;; 183) echo aarch64 ;; *) echo unknown ;; esac ;;
    macho) m="$(read_le "$bin" 4 4)"
           case "$m" in 16777223) echo x64 ;; 16777228) echo aarch64 ;; *) echo unknown ;; esac ;;
    *)     echo unknown ;;
  esac
}

# 校验 jlink 产物 <1> 是不是给 <2:os> / <3:arch> 用的。
# 只看 magic 会被**同格式但错架构**的产物骗过去（x64 的 jlink + aarch64 的 jmods 就
# 会产出这种 PE/ELF/Mach-O），装上就是 UnsatisfiedLinkError —— 所以格式与架构都要核。
assert_native_artifact() {
  local bin="$1" want_os="$2" want_arch="$3"
  local fmt arch
  fmt="$(binary_format "$bin")"
  case "$want_os:$fmt" in
    windows:pe|linux:elf|mac:macho) ;;
    mac:macho-fat)
      echo "    注意：$bin 是通用二进制（fat），跳过架构自检" >&2
      return 0 ;;
    windows:*)
      echo "错误：$bin 不是 PE（magic=$(binary_magic "$bin")），但目标是 windows。" >&2
      return 1 ;;
    linux:*)
      echo "错误：$bin 不是 ELF（magic=$(binary_magic "$bin")），但目标是 linux。" >&2
      return 1 ;;
    mac:*)
      echo "错误：$bin 不是 Mach-O（magic=$(binary_magic "$bin")），但目标是 mac。" >&2
      echo "      Windows=4d5a(PE) / Linux=7f454c46(ELF) / macOS=cffaedfe(Mach-O)" >&2
      return 1 ;;
  esac
  arch="$(binary_arch "$bin")"
  if [[ "$arch" != "$want_arch" ]]; then
    echo "错误：$bin 是 $fmt 但架构是 $arch，与目标 $want_arch 不符（jlink 交叉生成了？）。" >&2
    return 1
  fi
  return 0
}
host_os() {
  case "$(uname -s)" in
    Linux)                     echo linux ;;
    Darwin)                    echo mac ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT) echo windows ;;
    *)                         echo unknown ;;
  esac
}

# `$JAVA_HOME/release` 里 OS_ARCH 的写法各发行版不同：
# Temurin win-x64 是 `x86_64`、Linux 上是 `amd64`、Zulu win-arm64 是 `aarch64`。
# 两种写法都要认，别只匹配一种（那会让自检在某个发行版上悄悄退化成 uname）。
jdk_release_field() { # <字段名>
  local rel="${JAVA_HOME:-}/release"
  [[ -f "$rel" ]] || return 1
  sed -n "s/^$1=\"\{0,1\}\([^\"]*\)\"\{0,1\}$/\1/p" "$rel" | head -1
}

host_arch() {
  local a
  # 1) 宿主 JDK 的 java 二进制头（最准：那正是 jlink 会产出/依赖的架构）
  if [[ -n "${JAVA_HOME:-}" ]]; then
    local jb
    for jb in "$JAVA_HOME/bin/java" "$JAVA_HOME/bin/java.exe"; do
      [[ -f "$jb" ]] || continue
      a="$(binary_arch "$jb")"
      [[ "$a" != unknown ]] && { echo "$a"; return; }
    done
  fi
  # 2) JDK 的 release 元数据
  if a="$(jdk_release_field OS_ARCH)" && [[ -n "$a" ]]; then
    case "$a" in
      x86_64|amd64)  echo x64;     return ;;
      aarch64|arm64) echo aarch64; return ;;
    esac
  fi
  # 3) 最后才问 shell
  case "$(uname -m)" in
    x86_64|amd64)  echo x64 ;;
    aarch64|arm64) echo aarch64 ;;
    *)             echo unknown ;;
  esac
}

host_os_resolved() {
  # OS_NAME 比 `uname -s` 更贴近 jlink 的产出平台（同为 JDK 自带元数据）。
  local n
  if n="$(jdk_release_field OS_NAME)" && [[ -n "$n" ]]; then
    case "$n" in
      Windows) echo windows; return ;;
      Linux)   echo linux;   return ;;
      Darwin|Mac*) echo mac; return ;;
    esac
  fi
  host_os
}

# 宿主 JDK 自带的 jmods 目录（**MSYS 形式**）；没有就返回 1。
#
# 为什么不直接 `compgen -G "$JAVA_HOME/jmods/*.jmod"`：CI 上 `setup-java` 注入的
# `JAVA_HOME` 是 **Windows 形式**（`C:\hostedtoolcache\windows\Java_Zulu_jdk\
# 25.0.4-7\arm64`），而反斜杠在 bash 的**路径名展开**里是转义符 ——
#
#   [[ -d "C:\hostedtoolcache\...\arm64/jmods" ]]            → 真（MSYS 会归一化）
#   compgen -G "C:\hostedtoolcache\...\arm64/jmods/*.jmod"   → **假**（\U \J 被吃掉）
#
# 路径于是变成 `C:hostedtoolcachewindowsJava_Zulu_jdk25.0.4-7arm64/...`，永远匹配不到。
# 症状极隐蔽：**本地永远复现不了**（本机 Temurin 25 按 JEP 493 不带 jmods，无论如何都
# 走下载分支），只在 windows-arm64 上炸 —— Zulu 明明自带 70 个 .jmod，却被判成「没有」
# → 去 Adoptium 下 → 该平台 404 → 退出 1（run 35078586922）。
# 所以：先把 JAVA_HOME 归一化成 MSYS 路径再判，且用 `find` 而不是 glob
# （find 的参数不经路径名展开，不受反斜杠影响）。
jmods_dir() {
  local home="${JAVA_HOME:-}" d
  [[ -n "$home" ]] || return 1
  command -v cygpath >/dev/null 2>&1 && home="$(cygpath -u "$home")"
  d="$home/jmods"
  [[ -d "$d" ]] || return 1
  [[ -n "$(find "$d" -maxdepth 1 -name '*.jmod' -print -quit 2>/dev/null)" ]] || return 1
  printf '%s' "$d"
}

# <<< binary-probe <<<

HOST_OS="$(host_os_resolved)"
HOST_ARCH="$(host_arch)"

if [[ "$HOST_OS" != "$TARGET_OS" || "$HOST_ARCH" != "$TARGET_ARCH" ]]; then
  echo "错误：jlink 不能跨平台生成运行时（宿主 $HOST_OS/$HOST_ARCH，目标 $TARGET_OS/$TARGET_ARCH）。" >&2
  echo "      该目标的 +jre 必须跑在对应平台的原生 runner 上；" >&2
  echo "      见 .github/workflows/build.yml 里 targets mapping 的 os 列。" >&2
  exit 1
fi

# ---- 找 jlink（来自任意 JDK 25） ----------------------------------------

find_jlink() {
  local home="${JAVA_HOME:-}"
  local c
  for c in "$home/bin/jlink" "$home/bin/jlink.exe"; do
    [[ -x "$c" ]] && { echo "$c"; return 0; }
  done
  command -v jlink 2>/dev/null && return 0
  return 1
}

JLINK="$(find_jlink || true)"
if [[ -z "$JLINK" ]]; then
  echo "错误：找不到 jlink。装一个 JDK 25 并设置 JAVA_HOME（jlink 只随 JDK 提供，JRE 里没有）。" >&2
  exit 1
fi

PY="$(command -v python3 || command -v python)"
if [[ -z "$PY" ]]; then
  echo "错误：找不到 python3/python（解压 jmods 归档用）。" >&2
  exit 1
fi

# ---- 下载并解压 jmods ---------------------------------------------------

# Git Bash 给的是 `/tmp/...` `/e/...` 形式的 MSYS 路径，而 curl / python / jlink
# 都是**原生**程序，认不出这种写法（实测 curl 直接报 "Failed to open the file
# /tmp/..."）。传给它们之前统一转成 `C:\...` 形式；Linux/macOS 上没有 cygpath，
# 原样返回。bash 自己两种都认，所以转换后的路径全程通用。
native() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s' "$1"; fi
}

WORK="$(native "$(mktemp -d)")"
OUT="$(native "$OUT")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

JMODS_ARCHIVE="$WORK/jmods-archive"

# jmods 有两个来路，优先用宿主 JDK 自带的：
#
# 1. **宿主 JAVA_HOME/jmods/ 里有 .jmod** → 直接用，一次下载都不做。
# 2. 否则从 Adoptium 下（JEP 493 之后 Temurin 的归档不带 jmods/，所以默认走这条）。
#
# 为什么需要第 1 条：**Adoptium 对某些平台根本不发 JDK 25 的制品**。实测
# `windows/aarch64` 的 jdk / jre / jmods 全是 404（该平台只到 JDK 21），
# 那么 windows-arm64 的 +jre 就只能换一个带 jmods 的发行版 —— CI 上那个 target
# 用 Azul Zulu 装 JDK（它的 win_aarch64 归档自带 jmods/，实测 70 个 .jmod），
# 于是这里直接吃宿主自带的，不下载。
#
# 顺带的好处：jmods 与 jlink 来自同一个 JDK，同发行版同版本，不会有跨发行版
# 拼装带来的边角问题。
MODULE_PATH=""
if LOCAL_JMODS="$(jmods_dir)"; then
  MODULE_PATH="$(native "$LOCAL_JMODS")"
  echo "--- 用宿主 JDK 自带的 jmods（跳过下载）：$MODULE_PATH"
  echo "    jmod 数量：$(find "$LOCAL_JMODS" -maxdepth 1 -name '*.jmod' | wc -l | tr -d ' ')"
else
  echo "--- 下载 $TARGET_OS/$TARGET_ARCH 的 jmods"
  curl -fSL --retry 3 -o "$JMODS_ARCHIVE" \
    "https://api.adoptium.net/v3/binary/latest/25/ga/$TARGET_OS/$TARGET_ARCH/jmods/hotspot/normal/eclipse" \
    || {
      echo "错误：Adoptium 没有 $TARGET_OS/$TARGET_ARCH 的 jmods。" >&2
      echo "      该平台需要换一个带 jmods/ 的 JDK 发行版，并让 JAVA_HOME 指向它" >&2
      echo "      （见本脚本上面第 1 条；CI 里由 release.yml 的 jdk 列指定发行版）。" >&2
      exit 1
    }

  # `PYTHONUTF8=1`（Python 3.7+ 的 UTF-8 模式）+ `PYTHONIOENCODING` 一起强制 Python
  # 按 UTF-8 编码 stdout/stderr：Windows 上这两个流被重定向时走 **locale 编码**
  # （英文 runner 是 cp1252、中文机器是 cp936），而下面这段内联脚本会打中文
  # （"jmod 数量：…"），实测在 windows-latest 上直接
  # `UnicodeEncodeError: 'charmap' codec can't encode characters in position 9-11`
  # 退出 1。bash 自己的中文输出本来就是 UTF-8，这样两边才一致。
  PYTHONUTF8=1 PYTHONIOENCODING=utf-8 "$PY" - "$JMODS_ARCHIVE" "$WORK/jmods" <<'PY'
import sys, tarfile, zipfile, pathlib
arc, dest = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
dest.mkdir(parents=True, exist_ok=True)
# 归档里所有 jmod 都在同一个顶层目录下（windows 是 zip，linux/mac 是 tar.gz）
if zipfile.is_zipfile(arc):
    zipfile.ZipFile(arc).extractall(dest)
else:
    with tarfile.open(arc) as t:
        t.extractall(dest)
# 顶层那层目录就是 module-path（jmod 直接躺在它下面，没有 jmods/ 子目录）
tops = [p for p in dest.iterdir() if p.is_dir()]
root = tops[0] if len(tops) == 1 else dest
n = len(list(root.glob("*.jmod")))
if n == 0:
    sys.exit("错误：解压后 %s 下没有 .jmod 文件" % root)
print("    jmod 数量：%d（module-path: %s）" % (n, root))
pathlib.Path(dest / "MODULE_PATH").write_text(str(root))
PY

  MODULE_PATH="$(cat "$WORK/jmods/MODULE_PATH")"
fi

# ---- 白名单 ------------------------------------------------------------
#
# 每一行都对应"扩展/沙盒实际会碰到的 JDK 模块"。宁多勿少：少一个模块就是运行期
# NoClassDefFoundError，而多带的这些加起来也就几 MB。
#
# 实测依据：下面这份清单跑通了 tachiyomi-all.nhentaicom（加载 22 个 source，
# `/source/{id}/filters` 返回 13 个 filter —— 扩展自己的 getFilterList() 真实执行）。
MODULES="\
java.base,\
java.logging,\
java.management,\
java.naming,\
java.net.http,\
java.prefs,\
java.xml,\
jdk.crypto.ec,\
jdk.crypto.cryptoki,\
jdk.httpserver,\
jdk.localedata,\
jdk.management,\
jdk.unsupported,\
jdk.zipfs"

# 各模块为什么在（避免以后被"精简"掉）：
#   java.base          —— 必需。
#   java.logging       —— 扩展普遍用 java.util.logging / 日志桥。
#   java.management    —— SDK 常用 ManagementFactory 读内存/运行时信息。
#   java.naming        —— okhttp 的 DNS 与 TLS 会话路径会走到。
#   java.net.http      —— 部分扩展用 java.net.http.HttpClient 而不是 okhttp。
#   java.prefs         —— extension-runtime 的 PersistentCookieStore 用 java.util.prefs。
#   java.xml           —— 部分扩展解析 RSS/XML。
#   jdk.crypto.ec      —— TLS 的 ECDHE，缺了 HTTPS 直接握手失败。
#   jdk.crypto.cryptoki—— PKCS#11/证书链，与上面配套。
#   jdk.httpserver     —— **桌面沙盒自身的 HTTP 宿主**（com.sun.net.httpserver）。
#   jdk.localedata     —— 复数/日期/大小写规则；配 --include-locales 只留 en/ja/zh。
#   jdk.management     —— java.management 的扩展（MemoryPoolMXBean 等）。
#   jdk.unsupported    —— sun.misc.Unsafe（ASM / 反射 / Kotlin 运行时都会碰）。
#   jdk.zipfs          —— 扩展偶尔用 zip 文件系统读资源。
#
# **刻意排除 java.desktop**：AWT/Swing/ImageIO 一套约 11MB（压缩后），而扩展跑的是
# Android API（Bitmap 等由沙盒的 android.graphics 桩处理），用不到。若将来某个扩展
# 确实用了 java.awt / javax.imageio，把 java.desktop 加回上面的清单即可（会自动带上
# java.datatransfer）。

echo "--- jlink 生成运行时 -> $OUT"
rm -rf "$OUT"
"$JLINK" \
  --module-path "$MODULE_PATH" \
  --add-modules "$MODULES" \
  --include-locales=en,ja,zh \
  --output "$OUT" \
  --strip-debug \
  --no-man-pages \
  --no-header-files \
  --compress=zip-6

# ---- 自检 + 体积报告 ----------------------------------------------------

JAVA_BIN="$OUT/bin/java"
[[ -x "$JAVA_BIN" ]] || JAVA_BIN="$OUT/bin/java.exe"
if [[ ! -x "$JAVA_BIN" ]]; then
  echo "错误：$OUT/bin 下没有 java 可执行文件" >&2
  exit 1
fi

assert_native_artifact "$JAVA_BIN" "$TARGET_OS" "$TARGET_ARCH" || exit 1

SIZE="$(du -sh "$OUT" | cut -f1)"
echo "    完成：$OUT（$SIZE）"
"$JAVA_BIN" -version 2>&1 | head -1 | sed 's/^/    /'
