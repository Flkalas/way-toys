#!/bin/sh
# Reproduce the recorded KWin/X11 multi-output selection experiment.
set -eu

if [ "${1:-}" = "--inside-session" ]; then
    shift
else
    case "${1:-}" in equal|mixed) ;; *) echo "Usage: $0 equal|mixed [artifact-directory]" >&2; exit 2 ;; esac
    [ -n "${DISPLAY:-}" ] || { echo "DISPLAY must point to an X11 desktop" >&2; exit 2; }
    for cmd in dbus-run-session kwin_wayland kscreen-doctor wmctrl xdotool import awk stdbuf cargo git grep; do
        command -v "$cmd" >/dev/null 2>&1 || { echo "Missing command: $cmd" >&2; exit 2; }
    done
    xdotool getdisplaygeometry >/dev/null 2>&1 || { echo "Cannot access X11 DISPLAY; check DISPLAY and XAUTHORITY" >&2; exit 2; }
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    repo_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
    cargo build --manifest-path "$repo_dir/Cargo.toml" -p waytoys-overlay
    export WAYTOYS_REPO="$repo_dir"
    exec dbus-run-session -- "$script_dir/$(basename -- "$0")" --inside-session "$@"
fi

mode=$1
case "$mode" in equal) scale=1 ;; mixed) scale=1.5 ;; *) exit 2 ;; esac
artifacts=${2:-$(mktemp -d "${TMPDIR:-/tmp}/waytoys-repro-${mode}.XXXXXX")}
mkdir -p "$artifacts"
artifacts=$(CDPATH= cd -- "$artifacts" && pwd)
printf "Artifacts: %s\n" "$artifacts"
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/waytoys-runtime.XXXXXX")
chmod 700 "$test_dir"
export XDG_RUNTIME_DIR="$test_dir" XDG_CONFIG_HOME="$test_dir/config"
export XDG_CACHE_HOME="$test_dir/cache" XDG_DATA_HOME="$test_dir/data"
export WAYLAND_DISPLAY=waytoys-local
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME"
pid= app= button_down=0
cleanup() {
    if [ "$button_down" = 1 ]; then xdotool mouseup 1 >/dev/null 2>&1 || true; fi
    if [ -n "$app" ]; then kill "$app" 2>/dev/null || true; wait "$app" 2>/dev/null || true; fi
    if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; fi
    rm -rf -- "$test_dir"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM
printf 'mode=%s\ndisplay=%s\nsecond_scale=%s\n' "$mode" "$DISPLAY" "$scale" > "$artifacts/environment.log"
printf 'commit=' >> "$artifacts/environment.log"
git -C "$WAYTOYS_REPO" rev-parse HEAD >> "$artifacts/environment.log"
git -C "$WAYTOYS_REPO" status --short >> "$artifacts/environment.log"
kwin_wayland --version >> "$artifacts/environment.log" 2>&1 || true
kwin_wayland --x11-display "$DISPLAY" --socket "$WAYLAND_DISPLAY" --width 800 --height 600 \
    --output-count 2 --no-lockscreen --no-global-shortcuts --no-kactivities > "$artifacts/kwin.log" 2>&1 &
pid=$!
i=0
while [ ! -S "$test_dir/$WAYLAND_DISPLAY" ] && kill -0 "$pid" 2>/dev/null && [ "$i" -lt 100 ]; do sleep .1; i=$((i+1)); done
[ -S "$test_dir/$WAYLAND_DISPLAY" ] || { echo "KWin failed; see $artifacts/kwin.log" >&2; exit 1; }
export XDG_SESSION_TYPE=wayland QT_QPA_PLATFORM=wayland
kscreen-doctor output.X11-0.scale.1 "output.X11-1.scale.$scale" > "$artifacts/scale.log" 2>&1
# Preserve the historical wmctrl enumeration order; it is not a guaranteed mapping
# from a host X11 window to a nested Wayland output. Record it for diagnosis.
i=0
while [ "$i" -lt 50 ]; do
    wmctrl -lpG | awk -v id="$pid" '$3 == id' > "$artifacts/windows.log"
    [ "$(wc -l < "$artifacts/windows.log")" -ge 2 ] && break
    sleep .1; i=$((i+1))
done
first=$(awk 'NR == 1 {print $1}' "$artifacts/windows.log")
second=$(awk 'NR == 2 {print $1}' "$artifacts/windows.log")
[ -n "$first" ] && [ -n "$second" ] && [ "$first" != "$second" ] || { echo "Two nested windows not found" >&2; exit 1; }
printf 'first_window=%s\nsecond_window=%s\n' "$first" "$second" >> "$artifacts/environment.log"
xdotool windowsize "$second" 600 400
xdotool windowmove "$first" 700 200
xdotool windowmove "$second" 1600 200
sleep 2
kscreen-doctor -o > "$artifacts/outputs.log" 2>&1
wmctrl -lpG | awk -v id="$pid" '$3 == id' > "$artifacts/windows.log"
stdbuf -oL "$WAYTOYS_REPO/target/debug/waytoys-overlay" --select > "$artifacts/overlay.log" 2>&1 & app=$!
sleep 3
kill -0 "$app" 2>/dev/null || { echo "Overlay exited early; see $artifacts/overlay.log" >&2; exit 1; }
if [ "$mode" = equal ]; then expected='name=Some("X11-1") logical_size=Some((600, 400))'; else expected='name=Some("X11-1") logical_size=Some((400, 267))'; fi
if ! grep -F 'name=Some("X11-0") logical_size=Some((800, 600))' "$artifacts/overlay.log" >/dev/null || ! grep -F "$expected" "$artifacts/overlay.log" >/dev/null; then
    echo "INCONCLUSIVE: expected output geometry was not reported; see $artifacts/outputs.log and overlay.log" >&2
    exit 1
fi
xdotool windowactivate --sync "$second" mousemove --window "$second" 150 100 mousedown 1
button_down=1
xdotool mousemove --sync --window "$second" 450 300 sleep 1
import -window "$first" "$artifacts/first.png"
import -window "$second" "$artifacts/second.png"
xdotool mouseup 1
button_down=0
sleep 1
cat "$artifacts/windows.log" "$artifacts/overlay.log"
if ! grep -E '^selection output=[0-9]+ x=' "$artifacts/overlay.log" >/dev/null; then
    echo "INCONCLUSIVE: no selection event; see $artifacts/overlay.log" >&2
    exit 1
fi
printf 'Selection event recorded; compare coordinates and screen images with the historical record.\n'
printf '\nArtifacts: %s\n' "$artifacts"
