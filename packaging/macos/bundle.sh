#!/bin/bash
# Build Navifast.app from a GUI binary, on a macOS machine.
#
#   packaging/macos/bundle.sh <binary> <output.app> <version>
#
# Set CODESIGN_IDENTITY to select a certificate, or - for ad-hoc signing.
# Local checkouts can remember a certificate fingerprint in the ignored
# .release-secrets/macos-signing-identity file. CI uses explicit configuration.
#
# The .icns is generated here from the committed 1024px PNG, because iconutil
# only exists on macOS. The Info.plist template lives next to this script.
set -euo pipefail

binary="$1"
app="$2"
version="$3"
here="$(cd "$(dirname "$0")" && pwd)"

identity_file="$here/../../.release-secrets/macos-signing-identity"
if [[ ! ${CODESIGN_IDENTITY+x} && ${CI:-} != true && -f "$identity_file" ]]; then
    CODESIGN_IDENTITY="$(cat "$identity_file")"
    if [[ ! "$CODESIGN_IDENTITY" =~ ^[[:xdigit:]]{40}$ ]]; then
        echo "Expected a certificate SHA-1 fingerprint in $identity_file." >&2
        exit 1
    fi
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

executable=Navifast
identifier=io.github.nizarcan.Navifast
cp "$binary" "$app/Contents/MacOS/$executable"
chmod 755 "$app/Contents/MacOS/$executable"
# The build number has to be numbers: a release candidate's -rc1 comes off.
build="${version%%-*}"
sed -e "s/__VERSION__/$version/g" -e "s/__BUILD__/$build/g" \
    -e "s/__EXECUTABLE__/$executable/g" -e "s/__IDENTIFIER__/$identifier/g" "$here/Info.plist" \
    > "$app/Contents/Info.plist"

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
iconset="$scratch/navifast.iconset"
mkdir -p "$iconset"
# iconutil reads only these base sizes, each with an optional @2x. It ignores
# an icon_64x64 without saying so, so generating one is two wasted sips calls.
for size in 16 32 128 256 512; do
    sips -z $size $size "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/navifast.icns"

# arm64 refuses to launch an unsigned bundle, so sign one way or another.
if [[ -n ${CODESIGN_IDENTITY:-} && "$CODESIGN_IDENTITY" != - ]]; then
    signing=(--force --timestamp --options runtime --sign "$CODESIGN_IDENTITY")
    if [[ -n ${CODESIGN_KEYCHAIN:-} ]]; then
        signing+=(--keychain "$CODESIGN_KEYCHAIN")
    fi
    codesign "${signing[@]}" "$app"
else
    codesign --force --sign - "$app"
fi
codesign --verify --strict "$app"

echo "$app"
