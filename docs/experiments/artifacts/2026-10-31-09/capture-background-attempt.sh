#!/bin/sh
# Run in the real Plasma user's session, not inside a nested compositor.
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
artifacts=${1:?Usage: capture-overlay-clipboard.sh artifact-directory}
mkdir -p "$artifacts"
artifacts=$(CDPATH= cd -- "$artifacts" && pwd)
export QT_QPA_PLATFORM=wayland
for cmd in spectacle wl-paste wl-copy kscreen-doctor timeout; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "Missing command: $cmd" >&2; exit 2; }
done
app= watcher=
cleanup() {
    if [ -n "$watcher" ]; then
        kill "$watcher" 2>/dev/null || true
        wait "$watcher" 2>/dev/null || true
    fi
    if [ -n "$app" ]; then
        kill "$app" 2>/dev/null || true
        wait "$app" 2>/dev/null || true
    fi
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM
date -Is > "$artifacts/time.log"
git -C "$repo_dir" rev-parse HEAD > "$artifacts/commit.log"
kscreen-doctor -j > "$artifacts/outputs-before.json"
"$repo_dir/target/debug/waytoys-overlay" --display > "$artifacts/overlay.log" 2>&1 &
app=$!
i=0
while [ "$(grep -c '^surface configured output=' "$artifacts/overlay.log" || true)" -lt 2 ]; do
    kill -0 "$app" 2>/dev/null || { cat "$artifacts/overlay.log" >&2; exit 1; }
    i=$((i+1))
    [ "$i" -lt 100 ] || { echo "Two output surfaces were not configured" >&2; exit 1; }
    sleep .1
done
# Receive the PNG while Spectacle owns it, even if no clipboard manager keeps
# images after Spectacle exits. Preserve the exact bytes, then serve them again.
wl-copy --clear
timeout 25s wl-paste --type image/png --watch sh -c '
    case "${CLIPBOARD_STATE:-}" in data|sensitive) ;; *) exit 0 ;; esac
    cat > "$1"
    [ -s "$1" ] && touch "$2"
' sh \
    "$artifacts/spectacle-clipboard.png" "$artifacts/clipboard.ready" \
    > "$artifacts/clipboard-watch.log" 2>&1 &
watcher=$!
sleep .2
timeout 20s spectacle --background --fullscreen --copy-image --nonotify > "$artifacts/spectacle.log" 2>&1
i=0
while [ ! -f "$artifacts/clipboard.ready" ]; do
    i=$((i+1))
    [ "$i" -lt 50 ] || { echo "No screenshot received from clipboard" >&2; exit 1; }
    sleep .1
done
kill "$watcher" 2>/dev/null || true
wait "$watcher" 2>/dev/null || true
watcher=
wl-copy --type image/png < "$artifacts/spectacle-clipboard.png"
timeout 10s wl-paste --list-types > "$artifacts/clipboard-types.log"
grep -Fx 'image/png' "$artifacts/clipboard-types.log" >/dev/null || { echo "Clipboard has no PNG image" >&2; exit 1; }
timeout 10s wl-paste --type image/png > "$artifacts/clipboard.png"
signature=$(od -An -tx1 -N8 "$artifacts/clipboard.png" | tr -d ' \n')
[ "$signature" = '89504e470d0a1a0a' ] || { echo "Clipboard did not contain valid PNG data" >&2; exit 1; }
cmp "$artifacts/spectacle-clipboard.png" "$artifacts/clipboard.png"
kscreen-doctor -j > "$artifacts/outputs-after.json"
sha256sum "$artifacts/clipboard.png" > "$artifacts/clipboard.sha256"
printf 'Clipboard PNG: %s/clipboard.png\n' "$artifacts"
