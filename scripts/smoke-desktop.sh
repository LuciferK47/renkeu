#!/usr/bin/env bash
# Smoke-test a built desktop package: unpack it, start the *unpacked* app with
# no libpdfium anywhere on the loader path, compile a document through it and
# render a page. Passing proves the package carries a working libpdfium and that
# the app can run its own compile worker. Needs a display (use xvfb-run in CI).
#
#   scripts/smoke-desktop.sh path/to/Renkeu_0.1.0_amd64.deb
set -euo pipefail

DEB="${1:?usage: smoke-desktop.sh PACKAGE.deb}"
work="$(mktemp -d)"
app_pid=""
cleanup() {
  [ -n "$app_pid" ] && kill "$app_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

mkdir -p "$work/root" "$work/ws" "$work/home"
dpkg-deb -x "$DEB" "$work/root"
printf '\\documentclass{article}\n\\begin{document}\nSmoke test.\n\\end{document}\n' > "$work/ws/main.tex"

# The library must be in the package, where the shell looks for it.
bundled="$work/root/usr/lib/Renkeu/pdfium/libpdfium.so"
[ -f "$bundled" ] || { echo "FAIL: the package has no usr/lib/Renkeu/pdfium/libpdfium.so"; exit 1; }

# Start the app from the package, with nothing that could supply pdfium.
cd "$work/ws"
env -u LD_LIBRARY_PATH LATEX_AI_HOME="$work/home" RUST_LOG=info \
  "$work/root/usr/bin/latex-ai-tauri" > "$work/app.log" 2>&1 &
app_pid=$!

port=""
for _ in $(seq 1 60); do
  port="$(sed 's/\x1b\[[0-9;]*m//g' "$work/app.log" | grep -oE 'port=[0-9]+' | head -1 | cut -d= -f2 || true)"
  [ -n "$port" ] && break
  kill -0 "$app_pid" 2>/dev/null || { echo "FAIL: the app exited early:"; cat "$work/app.log"; exit 1; }
  sleep 1
done
[ -n "$port" ] || { echo "FAIL: the embedded API never came up:"; cat "$work/app.log"; exit 1; }
base="http://127.0.0.1:$port"

job="$(curl -fsS -X POST "$base/api/compile" -H 'content-type: application/json' -d '{"file_path":"main.tex"}')"
id="$(printf '%s' "$job" | sed -E 's/.*"job_id":"([^"]+)".*/\1/')"
# The first compile may download the TeX bundle, so be generous.
stream="$(curl -fsS -N --max-time 300 "$base/api/compile/stream/$id")"
printf '%s' "$stream" | grep -q '"success":true' || { echo "FAIL: compile did not succeed:"; echo "$stream"; exit 1; }

curl -fsS "$base/api/pdf/page?file=main.tex&n=1&dpi=96" -o "$work/page.png" \
  || { echo "FAIL: rendering a page failed (bundled pdfium not usable?):"; tail -5 "$work/app.log"; exit 1; }
[ "$(head -c 4 "$work/page.png" | tail -c 3)" = "PNG" ] || { echo "FAIL: the page is not a PNG"; exit 1; }

# And it really was the bundled copy that was loaded.
grep -q "$bundled" "/proc/$app_pid/maps" || { echo "FAIL: pdfium was not loaded from the package"; exit 1; }

echo "OK: compiled and rendered a page ($(wc -c < "$work/page.png") bytes) using the packaged libpdfium"
