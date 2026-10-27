#!/bin/sh
# Drives `pma web` in headless Chromium against a fresh fixture: builds pma,
# makes a web-fixture.sh store in a temporary directory, starts the server,
# runs web-check.mjs and web-check-runs.mjs, then stops the server and
# removes the directory. Fails when any check fails. `make web-check`.
#
# Needs node, and Chromium at $CHROME or in Playwright's cache.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
: "${CHROME:=$HOME/Library/Caches/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-mac-arm64/chrome-headless-shell}"
export CHROME
command -v node >/dev/null || { echo "web-check: node is not on PATH" >&2; exit 1; }
[ -x "$CHROME" ] || { echo "web-check: no Chromium at $CHROME; set CHROME" >&2; exit 1; }

cargo build --quiet --manifest-path "$root/Cargo.toml"
tmp="$(mktemp -d)"
server=""
cleanup() {
  [ -n "$server" ] && kill "$server" 2>/dev/null
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

sh "$root/scripts/web-fixture.sh" "$tmp/fx" >/dev/null
. "$tmp/fx/env.sh"
"$root/target/debug/pma" web --no-open > "$tmp/web.log" 2>&1 &
server=$!

# The server prints its address once it listens.
url=""
for _ in $(seq 50); do
  url="$(sed -n 's/^pma web: //p' "$tmp/web.log")"
  [ -n "$url" ] && break
  sleep 0.2
done
[ -n "$url" ] || { echo "web-check: pma web printed no address:" >&2; cat "$tmp/web.log" >&2; exit 1; }

status=0
node "$root/scripts/web-check.mjs" "$url" || status=1
node "$root/scripts/web-check-runs.mjs" "$url" "$tmp/fx/root/alpha" || status=1
exit $status
