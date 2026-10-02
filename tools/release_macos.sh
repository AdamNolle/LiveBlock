#!/usr/bin/env bash
# Build a universal Developer ID release. Optionally notarize and staple it.
# Usage: tools/release_macos.sh [--notarize KEYCHAIN_PROFILE]
# Optional: LB_RELEASE_IDENTITY="Developer ID Application: ..."
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE=""
case "${1:-}" in
  "") ;;
  --notarize) [[ $# -eq 2 && -n "$2" ]] || { echo "Expected a notarytool keychain profile" >&2; exit 2; }; PROFILE="$2" ;;
  *) echo "Usage: $0 [--notarize KEYCHAIN_PROFILE]" >&2; exit 2 ;;
esac
IDENTITY="${LB_RELEASE_IDENTITY:-}"
if [[ -z "$IDENTITY" ]]; then
  IDENTITIES=()
  while IFS= read -r identity; do IDENTITIES+=("$identity"); done < <(
    security find-identity -p codesigning -v | sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p'
  )
  [[ ${#IDENTITIES[@]} -eq 1 ]] || { echo "Set LB_RELEASE_IDENTITY to one valid Developer ID Application identity." >&2; exit 1; }
  IDENTITY="${IDENTITIES[0]}"
fi
RELEASE_DIR="${LB_RELEASE_DIR:-$HOME/Library/Caches/LiveBlock/release}"
mkdir -p "$RELEASE_DIR"
if [[ -n "$PROFILE" ]]; then
  # Check credentials before doing an expensive build. Never store secrets here.
  xcrun notarytool history --keychain-profile "$PROFILE" --output-format json > "$RELEASE_DIR/notary-history.json"
fi
LB_BRIDGE_PROFILE=release tools/build_bridge.sh
xcodegen generate
xcodebuild -project LiveBlock.xcodeproj -scheme LiveBlock -configuration Release \
  -destination 'platform=macOS' -derivedDataPath "$RELEASE_DIR/DerivedData" \
  'ARCHS=arm64 x86_64' ONLY_ACTIVE_ARCH=NO CODE_SIGNING_ALLOWED=NO \
  build > "$RELEASE_DIR/build.log" 2>&1 || { tail -60 "$RELEASE_DIR/build.log" >&2; exit 1; }
APP="$RELEASE_DIR/LiveBlock.app"
# Remove only the output bundle owned by this script; never touch installed apps.
rm -rf "$APP"
ditto "$RELEASE_DIR/DerivedData/Build/Products/Release/LiveBlock.app" "$APP"
ditto MODEL_CARD.md "$APP/Contents/Resources/MODEL_CARD.md"
ditto models/ATTRIBUTION.md "$APP/Contents/Resources/MODEL_ATTRIBUTION.md"
xattr -dr com.apple.FinderInfo "$APP" 2>/dev/null || true
xattr -dr com.apple.ResourceFork "$APP" 2>/dev/null || true
codesign --force --sign "$IDENTITY" --options runtime --timestamp "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
codesign -d --verbose=4 "$APP" 2> "$RELEASE_DIR/signature.txt"
lipo "$APP/Contents/MacOS/LiveBlock" -verify_arch arm64
lipo "$APP/Contents/MacOS/LiveBlock" -verify_arch x86_64
test -d "$APP/Contents/Resources/yolov8n.mlmodelc"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$RELEASE_DIR/LiveBlock-signed.zip"
shasum -a 256 "$RELEASE_DIR/LiveBlock-signed.zip" > "$RELEASE_DIR/LiveBlock-signed.zip.sha256"
if [[ -z "$PROFILE" ]]; then
  echo "Signed universal app: $APP"
  echo "Not notarized. Run: $0 --notarize <keychain-profile>"
  exit 0
fi
xcrun notarytool submit "$RELEASE_DIR/LiveBlock-signed.zip" \
  --keychain-profile "$PROFILE" --wait --output-format json > "$RELEASE_DIR/notary-result.json"
STATUS=$(plutil -extract status raw -o - "$RELEASE_DIR/notary-result.json")
SUBMISSION=$(plutil -extract id raw -o - "$RELEASE_DIR/notary-result.json")
if [[ "$STATUS" != "Accepted" ]]; then
  xcrun notarytool log "$SUBMISSION" --keychain-profile "$PROFILE" "$RELEASE_DIR/notary-log.json"
  echo "Notarization $STATUS; inspect $RELEASE_DIR/notary-log.json" >&2
  exit 1
fi
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=2 "$APP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$RELEASE_DIR/LiveBlock-notarized.zip"
shasum -a 256 "$RELEASE_DIR/LiveBlock-notarized.zip" > "$RELEASE_DIR/LiveBlock-notarized.zip.sha256"
echo "Notarized and stapled: $RELEASE_DIR/LiveBlock-notarized.zip"
