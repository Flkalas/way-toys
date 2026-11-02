set -eu
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland
for attempt in 1 2 3 4 5; do
    [ "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Settings1)" = false ] && break
    sleep 0.2
done
[ "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Settings1)" = false ]
systemd-run --user --unit=waytoys-settings-25 --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=ICED_BACKEND=tiny-skia /home/debian/projects/way-toys/target/debug/waytoys-settings --app-config-dir /home/debian/waytoys-test-results/settings-23/app --config-dir /home/debian/waytoys-test-results/settings-23/zones --page zones
sleep 1
cd /home/debian/projects/way-toys
python3 scripts/kwin-window-probe.py --caption Waytoys --app waytoys-settings --place 43 15 762 409
sleep 0.5
python3 scripts/kwin-window-probe.py --caption Waytoys --app waytoys-settings
systemctl --user show waytoys-runtime.service waytoys-settings-25.service -p MainPID -p ActiveState
