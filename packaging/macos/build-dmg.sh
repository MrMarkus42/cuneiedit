#!/bin/sh
# Builds target/dmg/CuneiEdit.app and CuneiEdit-<version>.dmg. Needs the Xcode command line tools
# (iconutil, codesign, hdiutil). The app always uses the hardened runtime. It is signed ad hoc
# unless CUNEIEDIT_SIGN_IDENTITY names a Developer ID certificate in the keychain; with APPLE_ID,
# APPLE_TEAM_ID and APPLE_APP_PASSWORD (app-specific password) it is also notarized.
set -eu
cd "$(dirname "$0")/../.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
OUT=target/dmg
APP="$OUT/CuneiEdit.app"
IDENTITY="${CUNEIEDIT_SIGN_IDENTITY:-}"

sign() {
    if [ -n "$IDENTITY" ]; then
        codesign --force --options runtime --timestamp --sign "$IDENTITY" "$1"
    else
        codesign --force --options runtime --sign - "$1"
    fi
}

# Notarizes and staples $1 (a .dmg, or an .app sent as a ZIP), if notarization is configured.
notarize() {
    if [ -z "$IDENTITY" ] || [ -z "${APPLE_ID:-}" ]; then
        return
    fi
    upload="$1"
    case "$1" in
    *.app)
        upload="$OUT/notarize.zip"
        ditto -c -k --keepParent "$1" "$upload"
        ;;
    esac
    xcrun notarytool submit "$upload" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" --wait
    [ "$upload" = "$1" ] || rm "$upload"
    xcrun stapler staple "$1"
}

cargo build --release -p cuneiedit-app
rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/cuneiedit "$APP/Contents/MacOS/CuneiEdit"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

cargo run --release -q -p cuneiedit-app --example app_icon -- "$OUT/CuneiEdit.iconset"
iconutil -c icns "$OUT/CuneiEdit.iconset" -o "$APP/Contents/Resources/CuneiEdit.icns"

sign "$APP"
notarize "$APP"

STAGE="$OUT/stage"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "CuneiEdit $VERSION" -srcfolder "$STAGE" -ov -format UDZO \
    "$OUT/CuneiEdit-$VERSION.dmg"
rm -rf "$STAGE" "$OUT/CuneiEdit.iconset"
if [ -n "$IDENTITY" ]; then
    sign "$OUT/CuneiEdit-$VERSION.dmg"
fi
notarize "$OUT/CuneiEdit-$VERSION.dmg"
echo "$OUT/CuneiEdit-$VERSION.dmg"
