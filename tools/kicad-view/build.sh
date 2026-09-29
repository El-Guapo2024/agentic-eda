#!/bin/sh
# Build KiCadView.app next to this script. Needs Xcode's Swift.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
app="$here/build/KiCadView.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$here/Info.plist" "$app/Contents/Info.plist"
cp "$here/viewer.html" "$app/Contents/Resources/viewer.html"
swiftc -O -swift-version 5 -import-objc-header "$here/CGVirtual.h" "$here/main.swift" -o "$app/Contents/MacOS/KiCadView" \
  -framework AppKit -framework ScreenCaptureKit -framework CoreImage -framework Network
# macOS keys the Screen Recording and Accessibility grants to the signature.
# Signed with an Apple Development identity if there is one, they survive a
# rebuild; ad hoc signed, every build is a new app to macOS and asks again.
id=$(security find-identity -v -p codesigning 2>/dev/null | awk '/Apple Development/ {print $2; exit}')
codesign --force --sign "${id:--}" "$app"
echo "built $app"
