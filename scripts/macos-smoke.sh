#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo 'This check needs macOS.' >&2; exit 1; }
staging="$(mktemp -d "${TMPDIR:-/tmp}/lennette-native.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
xcrun clang -fobjc-arc -fblocks -mmacosx-version-min=13.0 -Wall -Wextra -Werror \
    tests/macos-native.m -o "$staging/native-smoke" \
    -framework AppKit -framework Carbon -framework ScreenCaptureKit \
    -framework CoreMedia -framework CoreAudio -framework CoreGraphics
"$staging/native-smoke"
if [ "$#" -gt 0 ]; then
    "$1" --version
    "$1" --help >/dev/null
fi
plutil -lint packaging/macos/Info.plist
