#!/bin/sh
# Install only the version-bound Waytoys plugin. Run inside the KDE user session.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(kwin_wayland --version | awk '{print $NF}')
[ "$version" = 6.3.6 ] || { echo "KWin 6.3.6 is required (found $version); rebuild the adapter for your version." >&2; exit 1; }
cmake -S "$repo/kwin/zones" -B "$repo/target/kwin-zones" -G Ninja -DCMAKE_BUILD_TYPE=Debug
cmake --build "$repo/target/kwin-zones"
plugins=$(qtpaths6 --query QT_INSTALL_PLUGINS)
destination="$plugins/kwin/plugins/waytoys_zones_bridge.so"
# Never truncate a mapped library. Qt may retain the old code until next login.
qdbus6 org.kde.KWin /Plugins org.kde.KWin.Plugins.UnloadPlugin waytoys_zones_bridge || true
sudo install -D -m 755 "$repo/target/kwin-zones/waytoys_zones_bridge.so" "$destination.new"
sudo mv -- "$destination.new" "$destination"
echo "Installed $destination. Log out and back in after updating an already loaded plugin."
