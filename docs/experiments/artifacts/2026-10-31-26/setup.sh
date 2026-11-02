set -eu
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus WAYLAND_DISPLAY=wayland-0 QT_QPA_PLATFORM=wayland
cd /home/debian/projects/way-toys
mkdir -p /home/debian/waytoys-test-results/capture-26
sha256sum /home/debian/waytoys-test-results/settings-23/app/settings.json /home/debian/waytoys-test-results/settings-23/zones/preferences.json /home/debian/waytoys-test-results/settings-23/zones/outputs/*.json "$HOME/.config/kwinrc" > /home/debian/waytoys-test-results/capture-26/before.sha256
qdbus6 --literal org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel.getGlobalShortcutsByKey 301989971
sh scripts/install-capture.sh --shortcut
target/debug/waytoys --quit
for attempt in 1 2 3 4 5 6 7 8 9 10; do
 [ "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Runtime1)" = false ] && break
 sleep 0.2
done
[ "$(qdbus6 org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner org.waytoys.Runtime1)" = false ]
sudo -n loginctl unlock-session 769
systemd-run --user --unit=waytoys-settings-26 --collect --setenv=WAYLAND_DISPLAY=wayland-0 --setenv=ICED_BACKEND=tiny-skia /home/debian/projects/way-toys/target/debug/waytoys-settings --app-config-dir /home/debian/waytoys-test-results/settings-23/app --config-dir /home/debian/waytoys-test-results/settings-23/zones --page capture
sleep 1
target/debug/waytoys-capture --check
sleep 1
target/debug/waytoys --status
