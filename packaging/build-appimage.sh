#!/bin/sh
# Builds an AppImage of Territoria: target/Territoria-<version>-<arch>.AppImage
# One file that runs on most Linux distributions without installing.
# Uses appimagetool from $APPIMAGETOOL, from PATH, or downloads a fixed
# release into target/. Run from anywhere.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
arch=$(uname -m)
cargo build --release -p territoria-app

tool=${APPIMAGETOOL:-$(command -v appimagetool || true)}
if [ -z "$tool" ]; then
    tool="target/appimagetool-1.9.1-$arch.AppImage"
    if [ ! -x "$tool" ]; then
        curl -fL -o "$tool.part" \
            "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$arch.AppImage"
        chmod +x "$tool.part"
        mv "$tool.part" "$tool"
    fi
fi

appdir=$(mktemp -d)
trap 'rm -rf "$appdir"' EXIT
chmod 755 "$appdir"
install -Dm755 target/release/territoria "$appdir/usr/bin/territoria"
install -Dm644 packaging/territoria.desktop "$appdir/usr/share/applications/territoria.desktop"
install -Dm644 packaging/territoria.svg "$appdir/usr/share/icons/hicolor/scalable/apps/territoria.svg"
install -Dm644 packaging/territoria-mime.xml "$appdir/usr/share/mime/packages/territoria.xml"
install -Dm644 README.md "$appdir/usr/share/doc/territoria/README.md"
install -Dm644 docs/guide.md "$appdir/usr/share/doc/territoria/guide.md"
install -Dm644 docs/tutorial/customers.csv "$appdir/usr/share/doc/territoria/tutorial/customers.csv"
# appimagetool wants the entry and icon at the top as well.
cp packaging/territoria.desktop "$appdir/territoria.desktop"
cp packaging/territoria.svg "$appdir/territoria.svg"
ln -s territoria.svg "$appdir/.DirIcon"
# Graphics (Vulkan, OpenGL, Wayland, X11) come from the system, as they
# must: the program loads them at run time.
cat > "$appdir/AppRun" <<'APPRUN'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
exec "$here/usr/bin/territoria" "$@"
APPRUN
chmod 755 "$appdir/AppRun"

out="target/Territoria-$version-$arch.AppImage"
# Without FUSE (containers, CI) the tool can still run unpacked.
ARCH=$arch APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$appdir" "$out"
echo "$out"
