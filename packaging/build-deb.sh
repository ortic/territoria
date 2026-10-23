#!/bin/sh
# Builds a Debian package of Territoria: target/territoria_<version>_<arch>.deb
# Needs dpkg-deb (Debian, Ubuntu). Run from anywhere.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
arch=$(dpkg --print-architecture)
cargo build --release -p territoria-app
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
chmod 755 "$stage"
install -Dm755 target/release/territoria "$stage/usr/bin/territoria"
install -Dm644 packaging/territoria.desktop "$stage/usr/share/applications/territoria.desktop"
install -Dm644 packaging/territoria.svg "$stage/usr/share/icons/hicolor/scalable/apps/territoria.svg"
install -Dm644 packaging/territoria-mime.xml "$stage/usr/share/mime/packages/territoria.xml"
install -Dm644 README.md "$stage/usr/share/doc/territoria/README.md"
install -Dm644 docs/guide.md "$stage/usr/share/doc/territoria/guide.md"
install -Dm644 docs/tutorial/customers.csv "$stage/usr/share/doc/territoria/tutorial/customers.csv"
size=$(du -sk "$stage/usr" | cut -f1)
mkdir -p "$stage/DEBIAN"
cat > "$stage/DEBIAN/control" <<CONTROL
Package: territoria
Version: $version
Section: science
Priority: optional
Architecture: $arch
Installed-Size: $size
Depends: libc6, libgcc-s1, libxkbcommon0, libwayland-client0 | libx11-6
Recommends: libvulkan1, mesa-vulkan-drivers | libgl1
Maintainer: Remo Laubacher <remo.laubacher@ortic.com>
Homepage: https://github.com/ortic/territoria
Description: Geomarketing and territory planning
 Put your data on the map and plan territories that are fair and compact:
 boundaries from Shapefile, GeoJSON, GeoPackage or KML, tables from CSV or
 Excel, maps coloured by value, charts and heat maps, and sales
 territories balanced by any measure.
CONTROL
out="target/territoria_${version}_${arch}.deb"
dpkg-deb --root-owner-group --build "$stage" "$out"
echo "$out"
