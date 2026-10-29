#!/bin/bash
# Package an existing native binary; does not install it or alter user permissions.
set -euo pipefail
cd "$(dirname "$0")/.."
binary="${1:-target/release/lennette}"
platform="${2:-macos-$(uname -m)}"
case "$platform" in
    macos-arm64|macos-aarch64) platform=macos-aarch64; arch=arm64 ;;
    macos-x86_64) arch=x86_64 ;;
    *) echo "Unsupported platform: $platform" >&2; exit 1 ;;
esac
[ -f "$binary" ] || { echo "Build the macOS binary first: cargo build --locked --release" >&2; exit 1; }
lipo "$binary" -verify_arch "$arch"
# Work in an isolated temporary directory so reruns never delete user files.
staging="$(mktemp -d "${TMPDIR:-/tmp}/lennette-package.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
app="$staging/Lennette.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$staging/Lennette.iconset" dist
cp "$binary" "$app/Contents/MacOS/lennette"
chmod +x "$app/Contents/MacOS/lennette"
cp packaging/macos/Info.plist "$app/Contents/Info.plist"
python3 scripts/check-app-identity.py "$binary" --write-macos-plist "$app/Contents/Info.plist"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" src/assets/lennette_app_icon.png --out "$staging/Lennette.iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" src/assets/lennette_app_icon.png --out "$staging/Lennette.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$staging/Lennette.iconset" -o "$app/Contents/Resources/Lennette.icns"
cp src/assets/lennette_app_icon.png "$app/Contents/Resources/"
cp LICENSE "$app/Contents/Resources/"
cp .github/README.md "$app/Contents/Resources/README.md"
plutil -lint "$app/Contents/Info.plist"
python3 scripts/check-app-identity.py "$binary" --bundle "$app"
# Ad-hoc signing works without a developer account. Distribution with trusted
# Gatekeeper identity/notarization is a separate maintainer release step.
codesign --force --sign "${MACOS_SIGN_IDENTITY:--}" "$app"
codesign --verify --strict --verbose=2 "$app"
"$app/Contents/MacOS/lennette" --version
ditto -c -k --sequesterRsrc --keepParent "$app" "dist/lennette-${platform}.zip"
echo "Created dist/lennette-${platform}.zip"

# Familiar Mac installation: a branded volume with the app and Applications link.
image="$staging/image"
mkdir -p "$image"
ditto "$app" "$image/Lennette.app"
ln -s /Applications "$image/Applications"
cp "$app/Contents/Resources/Lennette.icns" "$image/.VolumeIcon.icns"
xcrun SetFile -a C "$image"
hdiutil create -volname Lennette -srcfolder "$image" -format UDZO -ov "dist/lennette-${platform}.dmg"
echo "Created dist/lennette-${platform}.dmg"
