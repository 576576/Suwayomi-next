#!/usr/bin/env bash
# Builds the Suwayomi desktop shell (suwayomi / suwayomi.exe) with a real version
# number injected into tauri.conf.json — on Windows that lands in the PE version
# resource, elsewhere it is just the app metadata. Cross-platform.
#
# The version follows the same scheme as suwayomi-core/build.rs:
#   versionCode = git commit count + 3000  →  split into semver segments
#   "3119" → "3.1.1" (tauri-build requires a 3-segment version; on Windows the
#   FileVersion/ProductVersion strings show the same digits).
#
# Usage: bash build-tray.sh  (from anywhere; runs cargo build --release)

set -euo pipefail
cd "$(dirname "$0")"

# --- derive version code from git (same as suwayomi-core/build.rs) ---
COUNT="$(git rev-list --count HEAD 2>/dev/null || echo 38)"
VCODE=$((COUNT + 3000))
# split the versionCode into semver segments: "3119" → "3.1.1"
# (versionCode is always >= 3000 so it has ≥4 digits; take the first three)
VS="${VCODE}"
VER="${VS:0:1}.${VS:1:1}.${VS:2:1}"
echo "[build-tray] versionCode=${VCODE} -> version ${VER}"

# --- inject version into tauri.conf.json, restore afterwards (even on failure) ---
CONF="tauri.conf.json"
cp "$CONF" "$CONF.bak"
restore() {
  mv "$CONF.bak" "$CONF"
  echo "[build-tray] tauri.conf.json restored"
}
trap restore EXIT

# Linux runner 只有 python3，Windows runner 是 python——两者都兼容
PY="python3"
command -v "$PY" >/dev/null 2>&1 || PY="python"
"$PY" - "$VER" <<'PYEOF'
import json, sys
ver = sys.argv[1]
p = "tauri.conf.json"
d = json.load(open(p, encoding="utf-8"))
d["version"] = ver
with open(p, "w", encoding="utf-8") as f:
    json.dump(d, f, indent=2, ensure_ascii=False)
    f.write("\n")
print(f"[build-tray] version injected: {ver}")
PYEOF

# --- build ---
cargo build --release
# 产物名：Windows 带 .exe，Linux/macOS 无后缀
OUT="target/release/suwayomi"
[ -f "${OUT}.exe" ] && OUT="${OUT}.exe"
echo "[build-tray] done: ${OUT}"
