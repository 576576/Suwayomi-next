#!/usr/bin/env bash
# Builds the Suwayomi tray shell (suwayomi.exe) with a real version number
# injected into the Windows version resource.
#
# The version follows the same scheme as suwayomi-core/build.rs:
#   versionCode = git commit count + 3000  →  Windows version "3118.0.0"
# (server reports this as "r3118"; the tray keeps the same numeric code.)
#
# Usage: bash build-tray.sh  (from anywhere; runs cargo build --release)

set -euo pipefail
cd "$(dirname "$0")"

# --- derive version code from git (same as suwayomi-core/build.rs) ---
COUNT="$(git rev-list --count HEAD 2>/dev/null || echo 38)"
VCODE=$((COUNT + 3000))
VER="${VCODE}.0.0"
echo "[build-tray] versionCode=${VCODE} -> Windows version ${VER}"

# --- inject version into tauri.conf.json, restore afterwards (even on failure) ---
CONF="tauri.conf.json"
cp "$CONF" "$CONF.bak"
restore() {
  mv "$CONF.bak" "$CONF"
  echo "[build-tray] tauri.conf.json restored"
}
trap restore EXIT

python - "$VER" <<'PYEOF'
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
echo "[build-tray] done: target/release/suwayomi.exe"
