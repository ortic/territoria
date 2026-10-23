#!/bin/sh
# Builds a macOS disk image of Territoria: target/Territoria-<version>.dmg
# with Territoria.app (universal: Apple silicon and Intel) and a link to
# Applications. Needs macOS with Xcode command line tools and both Rust
# targets (rustup target add aarch64-apple-darwin x86_64-apple-darwin).
# Set CODESIGN_IDENTITY to sign the app ("Developer ID Application: …").
# Run from anywhere.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release -p territoria-app --target "$target"
done

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
app="$stage/Territoria.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/territoria" \
    target/aarch64-apple-darwin/release/territoria \
    target/x86_64-apple-darwin/release/territoria

# The icon, in every size macOS asks for.
iconset="$stage/territoria.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" packaging/territoria-1024.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" packaging/territoria-1024.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns -o "$app/Contents/Resources/territoria.icns" "$iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Territoria</string>
    <key>CFBundleDisplayName</key><string>Territoria</string>
    <key>CFBundleIdentifier</key><string>com.ortic.territoria</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>territoria</string>
    <key>CFBundleIconFile</key><string>territoria</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.business</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeName</key><string>Territoria project</string>
            <key>CFBundleTypeRole</key><string>Editor</string>
            <key>CFBundleTypeIconFile</key><string>territoria</string>
            <key>LSHandlerRank</key><string>Owner</string>
            <key>LSItemContentTypes</key>
            <array><string>com.ortic.territoria.project</string></array>
        </dict>
    </array>
    <key>UTExportedTypeDeclarations</key>
    <array>
        <dict>
            <key>UTTypeIdentifier</key><string>com.ortic.territoria.project</string>
            <key>UTTypeDescription</key><string>Territoria project</string>
            <key>UTTypeConformsTo</key><array><string>public.data</string></array>
            <key>UTTypeTagSpecification</key>
            <dict>
                <key>public.filename-extension</key><array><string>territoria</string></array>
            </dict>
        </dict>
    </array>
</dict>
</plist>
PLIST

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    codesign --force --options runtime --timestamp --sign "$CODESIGN_IDENTITY" "$app"
fi

# The guide beside the app, and a link to drag it onto.
mkdir "$stage/dmg"
mv "$app" "$stage/dmg/"
cp docs/guide.md "$stage/dmg/Guide.md"
ln -s /Applications "$stage/dmg/Applications"
out="target/Territoria-$version.dmg"
rm -f "$out"
hdiutil create -volname "Territoria $version" -srcfolder "$stage/dmg" -fs HFS+ -format UDZO "$out"
echo "$out"
