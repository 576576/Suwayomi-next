#!/usr/bin/env bash
# 交叉编译 Rust server 为 Android 的 cdylib，并放到 :app 的 jniLibs 里。
#
# 用法：
#   android/scripts/build-rust.sh                      # 默认 arm64（发布走这条）
#   ABI=x86_64 android/scripts/build-rust.sh           # 模拟器验证用
#   ABI=all android/scripts/build-rust.sh              # 两个都出
#   PROFILE=debug android/scripts/build-rust.sh        # 快速验证工具链
#
# 为什么默认只有 arm64：计划里的 Android 构建目标就是 arm64（真机）。
# x86_64 只为了在 x86_64 模拟器上跑端到端验证，不进发布产物。
#
# NDK 位置按顺序解析：$ANDROID_NDK_HOME → $ANDROID_HOME/ndk/<最大版本> →
# $ANDROID_SDK_ROOT/ndk/<最大版本>。找不到就报错退出——不要静默回退到主机 cc，
# 那样会产出一个 x86_64-linux 的 .so 打进 APK，装到手机上是 UnsatisfiedLinkError。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# 与 android/app/build.gradle.kts 的 minSdk 保持一致
API=26
PROFILE="${PROFILE:-android-release}"
ABI="${ABI:-arm64}"

case "$ABI" in
  arm64)  ABIS="arm64" ;;
  x86_64) ABIS="x86_64" ;;
  all)    ABIS="arm64 x86_64" ;;
  *) echo "错误：ABI 应为 arm64|x86_64|all，收到 '$ABI'" >&2; exit 1 ;;
esac

resolve_ndk() {
  if [[ -n "${ANDROID_NDK_HOME:-}" && -d "$ANDROID_NDK_HOME" ]]; then
    echo "$ANDROID_NDK_HOME"; return
  fi
  local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
  # Windows 的 SDK 路径常带反斜杠（C:\Users\...\Sdk）；rustc/cc 都是原生进程，
  # 需要正斜杠形式，bash 的 -d/ls 也认这种写法，所以统一在入口处归一化。
  sdk="${sdk//\\//}"
  [[ -n "$sdk" && -d "$sdk/ndk" ]] || return 1
  # 目录名是版本号，按版本排序取最大（sort -V 对 28.2.13676358 这种多段数字正确）
  ls -1 "$sdk/ndk" | grep -v '^\.' | sort -V | tail -1 | sed "s|^|$sdk/ndk/|"
}

NDK="$(resolve_ndk)" || {
  echo "错误：找不到 Android NDK。" >&2
  echo "  设置 ANDROID_NDK_HOME，或用 sdkmanager 安装：sdkmanager 'ndk;28.2.13676358'" >&2
  exit 1
}

# 宿主平台决定 prebuilt 目录名（windows-x86_64 / linux-x86_64 / darwin-x86_64）
BIN=""
for p in windows-x86_64 linux-x86_64 darwin-x86_64 darwin-arm64; do
  if [[ -d "$NDK/toolchains/llvm/prebuilt/$p/bin" ]]; then BIN="$NDK/toolchains/llvm/prebuilt/$p/bin"; break; fi
done
[[ -n "$BIN" ]] || { echo "错误：$NDK 下找不到 llvm prebuilt 工具链" >&2; exit 1; }

echo "NDK     : $NDK"
echo "profile : $PROFILE"
echo "ABI     : $ABIS"

cd "$REPO_ROOT"

for a in $ABIS; do
  case "$a" in
    arm64)  TARGET="aarch64-linux-android"; ABI_DIR="arm64-v8a" ;;
    x86_64) TARGET="x86_64-linux-android";  ABI_DIR="x86_64" ;;
  esac

  CLANG="$BIN/$TARGET$API-clang"
  # Windows 的 NDK 同时放了三个同名文件：无后缀（sh 脚本，不能 exec）、.cmd、.exe。
  # 无后缀那个**存在**，所以不能简单地「有就用」——否则 rustc 会拿到一个
  # sh 脚本去 CreateProcess，报 os error 193（不是有效的 Win32 应用程序）。
  # 优先 .cmd/.exe（Windows），都没有才用无后缀（Linux/macOS）。
  if [[ -f "$CLANG.cmd" ]]; then
    CLANG="$CLANG.cmd"
  elif [[ -f "$CLANG.exe" ]]; then
    CLANG="$CLANG.exe"
  elif [[ -f "$CLANG" ]]; then
    :
  else
    echo "错误：$NDK 下找不到 $TARGET$API-clang" >&2
    exit 1
  fi

  # cc-rs 的环境变量名把 '-' 换成 '_'，且要全大写目标段
  SUFFIX="$(echo "$TARGET" | tr '-' '_')"
  SUFFIX_UPPER="$(echo "$SUFFIX" | tr '[:lower:]' '[:upper:]')"

  export "CARGO_TARGET_${SUFFIX_UPPER}_LINKER=$CLANG"
  export "CC_$SUFFIX=$CLANG"
  export "CXX_$SUFFIX=${CLANG/-clang/-clang++}"
  AR="$BIN/llvm-ar"
  [[ -f "$AR" ]] || AR="$BIN/llvm-ar.exe"
  export "AR_$SUFFIX=$AR"

  echo "--- $a -> $TARGET (clang: $CLANG)"
  cargo build -p suwayomi-android --target "$TARGET" --profile "$PROFILE"

  OUT_DIR="$REPO_ROOT/android/app/src/main/jniLibs/$ABI_DIR"
  mkdir -p "$OUT_DIR"
  cp -f "target/$TARGET/$PROFILE/libsuwayomi_android.so" "$OUT_DIR/"
  echo "    已拷贝 -> $OUT_DIR/libsuwayomi_android.so ($(du -h "$OUT_DIR/libsuwayomi_android.so" | cut -f1))"
done
