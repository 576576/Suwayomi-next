#!/usr/bin/env bash
# Export the Kotlin (Suwayomi-Server) GraphQL schema as SDL, saved to
# docs/graphql/schema-baseline.graphql. Run this once from an environment
# where the Kotlin server can be built & started; the result is the
# compatibility baseline used by Phase 4 (async-graphql must diff clean).
#
# Usage:
#   ./scripts/export-graphql-schema.sh [port]
set -euo pipefail

PORT="${1:-4567}"
WORKDIR="$(mktemp -d)"
SERVER_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KOTLIN_REPO="${KOTLIN_REPO:-$SERVER_ROOT/../Suwayomi-Server}"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then kill "$SERVER_PID" 2>/dev/null || true; fi
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

echo "==> Building Kotlin server (installDist)..."
(cd "$KOTLIN_REPO" && ./gradlew :server:installDist --console=plain -q)

DIST_BIN="$(find "$KOTLIN_REPO/server/build/install" -type f -name 'server' | head -1)"
if [[ -z "$DIST_BIN" ]]; then
  echo "ERROR: server binary not found under server/build/install" >&2
  exit 1
fi

echo "==> Starting server on port $PORT (headless config)..."

INTROSPECTION_QUERY='{"query":"query { __schema { queryType { name } mutationType { name } subscriptionType { name } types { kind name description fields(includeDeprecated: true) { name description args { name description type { kind name ofType { kind name ofType { kind name ofType { kind name } } } } defaultValue } type { kind name ofType { kind name ofType { kind name ofType { kind name } } } } isDeprecated deprecationReason } inputFields { name description type { kind name ofType { kind name ofType { kind name ofType { kind name } } } } defaultValue } interfaces { kind name } enumValues(includeDeprecated: true) { name description isDeprecated deprecationReason } possibleTypes { kind name } } directives { name description locations args { name description type { kind name ofType { kind name ofType { kind name ofType { kind name } } } } defaultValue } } } }"}'

mkdir -p "$SERVER_ROOT/docs/graphql"

# Start in background; disable browser/tray via config overrides where possible.
# NOTE: Suwayomi-Server reads its config from server.conf / system properties;
# adjust flags to match the installed version if needed.
(
  cd "$KOTLIN_REPO"
  SERVER_PORT="$PORT" "$DIST_BIN" --headless > "$WORKDIR/server.log" 2>&1
) &
SERVER_PID=$!

for i in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:$PORT/api/graphql" -H 'content-type: application/json' \
      -d "$INTROSPECTION_QUERY" -o "$WORKDIR/schema.json" 2>/dev/null; then
    break
  fi
  sleep 2
done

if [[ ! -s "$WORKDIR/schema.json" ]]; then
  echo "ERROR: server did not become ready (see log):" >&2
  tail -30 "$WORKDIR/server.log" >&2 || true
  exit 1
fi

# Convert introspection JSON -> SDL using the first available tool
if command -v npx >/dev/null 2>&1 && npx --yes graphql-cli get-schema --help >/dev/null 2>&1; then
  echo "==> Converting via graphql-cli..."
  npx --yes graphql-cli get-schema --endpoint "http://127.0.0.1:$PORT/api/graphql" \
    --output "$SERVER_ROOT/docs/graphql/schema-baseline.graphql" --no-schema
elif command -v python3 >/dev/null 2>&1; then
  echo "==> Converting via python (graphql-core)..."
  python3 - "$WORKDIR/schema.json" "$SERVER_ROOT/docs/graphql/schema-baseline.graphql" <<'PY'
import json, sys
try:
    from graphql import build_client_schema, print_schema
except ImportError:
    print("graphql-core not installed; run: pip install graphql-core", file=sys.stderr)
    sys.exit(2)
with open(sys.argv[1]) as f:
    data = json.load(f)
schema = build_client_schema(data.get("data", data).get("__schema", data))
with open(sys.argv[2], "w") as f:
    f.write(print_schema(schema))
PY
else
  echo "WARN: no converter available; raw introspection JSON kept at $WORKDIR/schema.json" >&2
  exit 3
fi

echo "==> Done: docs/graphql/schema-baseline.graphql"
