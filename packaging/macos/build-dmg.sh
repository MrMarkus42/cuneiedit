#!/bin/sh
# Builds target/dmg/Pilcrow.app and Pilcrow-<version>.dmg. Needs the Xcode command line tools
# (iconutil, codesign, hdiutil). The app is signed ad hoc only, not notarized.
set -eu
cd "$(dirname "$0")/../.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
OUT=target/dmg
APP="$OUT/Pilcrow.app"

cargo build --release -p pilcrow-app
rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/pilcrow "$APP/Contents/MacOS/Pilcrow"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

cargo run --release -q -p pilcrow-app --example app_icon -- "$OUT/Pilcrow.iconset"
iconutil -c icns "$OUT/Pilcrow.iconset" -o "$APP/Contents/Resources/Pilcrow.icns"

codesign --force --deep --sign - "$APP"

STAGE="$OUT/stage"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "Pilcrow $VERSION" -srcfolder "$STAGE" -ov -format UDZO \
    "$OUT/Pilcrow-$VERSION.dmg"
rm -rf "$STAGE" "$OUT/Pilcrow.iconset"
echo "$OUT/Pilcrow-$VERSION.dmg"
