set -eu
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland
cd /home/debian/projects/way-toys
test "$(qdbus6 org.waytoys.Settings1 /Settings org.waytoys.Settings1.Close '')" = true
target/debug/waytoys --quit
for attempt in $(seq 1 20); do
 if [ "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Runtime1)" = false ] && [ "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Paste1)" = false ]; then break; fi
 sleep 0.2
done
test "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Paste1)" = false
systemd-run --user --unit=waytoys-settings-27-final --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=ICED_BACKEND=tiny-skia /home/debian/projects/way-toys/target/debug/waytoys-settings --app-config-dir /home/debian/waytoys-test-results/settings-23/app --config-dir /home/debian/waytoys-test-results/settings-23/zones --page paste
sleep 2
target/debug/waytoys --status
