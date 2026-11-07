#!/bin/sh
# Builds the Rust app for iPhone, signs the shell in ios/ and installs and launches it on a
# connected iPhone. Run on the Mac from apps/mac:
#
#   scripts/iphone.sh            first connected iPhone
#   scripts/iphone.sh <udid>     a given one (xcrun devicectl list devices)
#
# Signing is headless with an App Store Connect API key described by a JSON file
# (keyId, issuerId, keyPath); set ASC_KEY_JSON to use another one.
set -eu
cd "$(dirname "$0")/.."

KEY_JSON=${ASC_KEY_JSON:-$HOME/.appstoreconnect/operalibre.json}
field() { python3 -c "import json,os,sys; print(os.path.expanduser(json.load(open(sys.argv[1]))[sys.argv[2]]))" "$KEY_JSON" "$1"; }

UDID=${1:-$(xcrun devicectl list devices 2>/dev/null | grep iPhone | grep -E 'connected|available' | grep -oE '[0-9A-F]{8}-[0-9A-F]{16}' | head -1)}
[ -n "$UDID" ] || { echo "No connected iPhone found." >&2; exit 1; }

echo "Building the Rust library"
# Match the app's deployment target, so C code in dependencies links without warnings.
export IPHONEOS_DEPLOYMENT_TARGET=18.0
cargo build -p open-notebook-mac --lib --release --target aarch64-apple-ios

echo "Building and signing the app"
xcodebuild -project ios/OpenNotebook.xcodeproj -scheme OpenNotebook -configuration Release \
  -destination "id=$UDID" -derivedDataPath ios/build \
  -allowProvisioningUpdates \
  -authenticationKeyPath "$(field keyPath)" -authenticationKeyID "$(field keyId)" -authenticationKeyIssuerID "$(field issuerId)" \
  build -quiet

APP=ios/build/Build/Products/Release-iphoneos/OpenNotebook.app
echo "Installing on $UDID"
xcrun devicectl device install app --device "$UDID" "$APP"
xcrun devicectl device process launch --device "$UDID" --terminate-existing com.donovanmontoya.opennotebook.rune
