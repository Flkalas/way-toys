#!/bin/sh
# Run the recorded X11 tests on a private X server, without moving host input.
set -eu

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
artifacts=${1:-$(mktemp -d "${TMPDIR:-/tmp}/waytoys-headless.XXXXXX")}
mkdir -p "$artifacts"
artifacts=$(CDPATH= cd -- "$artifacts" && pwd)
for cmd in xvfb-run openbox wmctrl xprop xdotool; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "Missing command: $cmd" >&2; exit 2; }
done
# KWin nested windows omit WM_CLIENT_MACHINE. This wmctrl build crashes when
# listing such windows; supply only that descriptive property in the private
# X server. The window ids, order, geometry, scale, and input are unchanged.
real_wmctrl=$(command -v wmctrl)
mkdir -p "$artifacts/bin"
cat > "$artifacts/bin/wmctrl" <<'EOF'
#!/bin/sh
for window in $(xprop -root _NET_CLIENT_LIST | sed -n 's/.*# //p' | tr ',' ' '); do
    if xprop -id "$window" WM_CLIENT_MACHINE 2>/dev/null | grep -q 'not found'; then
        xprop -id "$window" -f WM_CLIENT_MACHINE 8s -set WM_CLIENT_MACHINE localhost >/dev/null 2>&1 || true
    fi
done
exec "$WAYTOYS_REAL_WMCTRL" "$@"
EOF
chmod +x "$artifacts/bin/wmctrl"
export WAYTOYS_REAL_WMCTRL="$real_wmctrl"
export PATH="$artifacts/bin:$PATH"
printf 'Artifacts: %s\n' "$artifacts"
xvfb-run -a -s '-screen 0 3000x1600x24' sh -eu -c '
    repo_dir=$1
    artifacts=$2
    unset WAYLAND_DISPLAY
    openbox > "$artifacts/openbox.log" 2>&1 &
    wm_pid=$!
    trap '\''kill "$wm_pid" 2>/dev/null || true; wait "$wm_pid" 2>/dev/null || true'\'' EXIT
    i=0
    while ! wmctrl -m >/dev/null 2>&1; do
        i=$((i+1))
        [ "$i" -lt 50 ] || { echo "Openbox did not start" >&2; exit 1; }
        sleep .1
    done
    cd "$repo_dir"
    for mode in equal mixed; do
        if "$repo_dir/scripts/reproduce-nested-overlay.sh" "$mode" "$artifacts/$mode" > "$artifacts/$mode-run.log" 2>&1; then
            printf "%s exit=0\n" "$mode"
        else
            status=$?
            cat "$artifacts/$mode-run.log"
            printf "%s exit=%s\n" "$mode" "$status" >&2
            exit "$status"
        fi
        cat "$artifacts/$mode-run.log"
    done
' sh "$repo_dir" "$artifacts"
