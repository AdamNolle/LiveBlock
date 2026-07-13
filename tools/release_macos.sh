#!/usr/bin/env bash
# Build, Developer-ID sign, notarize, staple, and verify a macOS release.
# Default mode is credential-free dry-run. Nothing is signed or submitted until
# --execute is supplied with genuine credentials in the environment.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE=dry-run
OUTPUT_DIR=""
usage() {
  cat <<'EOF'
Usage: tools/release_macos.sh [--dry-run|--execute] [--output DIR]

Dry-run (default): validates local release tooling/project settings and prints
all signing/notarization commands without requiring credentials.

Execute requires:
  LB_DEVELOPER_ID_APPLICATION  Full Developer ID Application identity
  LB_TEAM_ID                   Apple Developer Team ID
  LB_NOTARY_PROFILE            notarytool keychain profile name
  LB_MACOS_MODEL_BUNDLE_DIR    Protected directory containing the precompiled
                               model, manifest, and trusted public keyring

Optional:
  ALLOW_DIRTY=1                Permit an execute-mode build from a dirty tree
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) MODE=dry-run; shift ;;
    --execute) MODE=execute; shift ;;
    --output) OUTPUT_DIR="${2:?--output requires a directory}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

for tool in xcodegen xcodebuild xcrun codesign ditto shasum git; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done

DEVELOPMENT_KEYRING="$ROOT/Sources/Resources/trusted-model-keys.json"
RELEASE_MODEL_STAGING="$ROOT/Sources/Resources/release-liveblock-detector.mlmodelc"
RELEASE_MANIFEST_STAGING="$ROOT/Sources/Resources/release-liveblock-detector.manifest.json"
RELEASE_KEYRING_STAGING="$ROOT/Sources/Resources/release-trusted-model-keys.json"
if [[ "$MODE" == execute ]]; then
  [[ "${LIVEBLOCK_ALLOW_EMPTY_MODEL_KEYRING:-0}" != 1 ]] || {
    echo "The empty model-keyring override is forbidden for release execution." >&2
    exit 1
  }
else
  /usr/bin/python3 tools/validate_model_keyring.py --keyring "$DEVELOPMENT_KEYRING" --allow-empty
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT/tools/runs/release-macos/$STAMP}"
ARCHIVE="$OUTPUT_DIR/LiveBlock.xcarchive"
APP="$ARCHIVE/Products/Applications/LiveBlock.app"
SUBMISSION_ZIP="$OUTPUT_DIR/LiveBlock-notary-submission.zip"
FINAL_ZIP="$OUTPUT_DIR/LiveBlock-macOS-notarized.zip"
NOTARY_JSON="$OUTPUT_DIR/notary-result.json"

print_command() {
  printf '  '
  printf '%q ' "$@"
  printf '\n'
}

archive_command=(
  xcodebuild -project LiveBlock.xcodeproj -scheme LiveBlock
  -configuration Release -archivePath "$ARCHIVE" archive
  CODE_SIGN_STYLE=Manual
  "CODE_SIGN_IDENTITY=${LB_DEVELOPER_ID_APPLICATION:-<Developer ID Application identity>}"
  "DEVELOPMENT_TEAM=${LB_TEAM_ID:-<Team ID>}"
  OTHER_CODE_SIGN_FLAGS=--timestamp
)

if [[ "$MODE" == dry-run ]]; then
  # Keep generated project metadata deterministic and prove Release settings load.
  xcodegen generate >/dev/null
  xcodebuild -project LiveBlock.xcodeproj -scheme LiveBlock -configuration Release \
    -showBuildSettings CODE_SIGNING_ALLOWED=NO >/dev/null
  cat <<EOF
macOS release dry-run passed local preflight.
No signing, keychain access, upload, or release artifact was performed.
The committed development model keyring is allowed only for this dry run;
execute mode requires a verified protected compiled-model bundle and nonempty ring.
Planned output: $OUTPUT_DIR
Planned commands:
EOF
  print_command "${archive_command[@]}"
  print_command codesign --verify --deep --strict --verbose=2 "$APP"
  print_command ditto -c -k --sequesterRsrc --keepParent "$APP" "$SUBMISSION_ZIP"
  print_command xcrun notarytool submit "$SUBMISSION_ZIP" --keychain-profile "${LB_NOTARY_PROFILE:-<notary profile>}" --wait --output-format json
  print_command xcrun stapler staple "$APP"
  print_command xcrun stapler validate "$APP"
  print_command spctl --assess --type execute --verbose=4 "$APP"
  print_command ditto -c -k --sequesterRsrc --keepParent "$APP" "$FINAL_ZIP"
  print_command shasum -a 256 "$FINAL_ZIP"
  exit 0
fi

: "${LB_DEVELOPER_ID_APPLICATION:?Set LB_DEVELOPER_ID_APPLICATION for --execute}"
: "${LB_TEAM_ID:?Set LB_TEAM_ID for --execute}"
: "${LB_NOTARY_PROFILE:?Set LB_NOTARY_PROFILE for --execute}"
: "${LB_MACOS_MODEL_BUNDLE_DIR:?Set LB_MACOS_MODEL_BUNDLE_DIR for --execute}"

if [[ "${ALLOW_DIRTY:-0}" != 1 ]] && [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
  echo "Refusing release from a dirty tree, including untracked files (set ALLOW_DIRTY=1 to override)." >&2
  exit 1
fi

MODEL_INPUT="$(cd "$LB_MACOS_MODEL_BUNDLE_DIR" && pwd)"
[[ "$MODEL_INPUT/liveblock-detector.mlmodelc" != "$RELEASE_MODEL_STAGING" ]] || {
  echo "LB_MACOS_MODEL_BUNDLE_DIR must not be the internal staging location." >&2; exit 1;
}
[[ -d "$MODEL_INPUT/liveblock-detector.mlmodelc" ]] || {
  echo "Missing precompiled liveblock-detector.mlmodelc in protected model bundle." >&2; exit 1;
}
[[ -f "$MODEL_INPUT/liveblock-detector.manifest.json" && ! -L "$MODEL_INPUT/liveblock-detector.manifest.json" ]] || {
  echo "Missing regular liveblock-detector.manifest.json in protected model bundle." >&2; exit 1;
}
/usr/bin/python3 tools/validate_model_keyring.py --keyring "$MODEL_INPUT/trusted-model-keys.json"
[[ -x "$ROOT/tools/.venv/bin/python" ]] || {
  echo "Protected release verification requires tools/.venv; install tools/requirements.txt." >&2; exit 1;
}
PYTHONPATH=tools "$ROOT/tools/.venv/bin/python" tools/verify_signed_model_bundle.py --bundle "$MODEL_INPUT"
rm -rf "$RELEASE_MODEL_STAGING"
rm -f "$RELEASE_MANIFEST_STAGING" "$RELEASE_KEYRING_STAGING"
cp -R "$MODEL_INPUT/liveblock-detector.mlmodelc" "$RELEASE_MODEL_STAGING"
cp "$MODEL_INPUT/liveblock-detector.manifest.json" "$RELEASE_MANIFEST_STAGING"
cp "$MODEL_INPUT/trusted-model-keys.json" "$RELEASE_KEYRING_STAGING"
cleanup_release_model_staging() {
  rm -rf "$RELEASE_MODEL_STAGING"
  rm -f "$RELEASE_MANIFEST_STAGING" "$RELEASE_KEYRING_STAGING"
}
trap cleanup_release_model_staging EXIT

xcodegen generate >/dev/null
xcodebuild -project LiveBlock.xcodeproj -scheme LiveBlock -configuration Release \
  -showBuildSettings CODE_SIGNING_ALLOWED=NO >/dev/null
mkdir -p "$OUTPUT_DIR"
"${archive_command[@]}"
[[ -d "$APP" ]] || { echo "Archive did not contain $APP" >&2; exit 1; }
APP_RESOURCES="$APP/Contents/Resources"
[[ -d "$APP_RESOURCES/release-liveblock-detector.mlmodelc" \
   && -f "$APP_RESOURCES/release-liveblock-detector.manifest.json" \
   && -f "$APP_RESOURCES/release-trusted-model-keys.json" ]] || {
  echo "Archive omitted authenticated production model resources." >&2; exit 1;
}

codesign --verify --deep --strict --verbose=2 "$APP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$SUBMISSION_ZIP"
xcrun notarytool submit "$SUBMISSION_ZIP" \
  --keychain-profile "$LB_NOTARY_PROFILE" \
  --wait --output-format json | tee "$NOTARY_JSON"
/usr/bin/python3 - "$NOTARY_JSON" <<'PY'
import json
import sys
from pathlib import Path

result = json.loads(Path(sys.argv[1]).read_text())
if result.get("status") != "Accepted":
    raise SystemExit(f"notarization was not accepted: {result.get('status', 'missing status')}")
PY
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=4 "$APP"

# The distributable must be created after stapling, not from the upload zip.
ditto -c -k --sequesterRsrc --keepParent "$APP" "$FINAL_ZIP"
shasum -a 256 "$FINAL_ZIP" | tee "$FINAL_ZIP.sha256"

GIT_COMMIT="$(git rev-parse HEAD)"
/usr/bin/python3 - "$OUTPUT_DIR/release-manifest.json" "$GIT_COMMIT" "$FINAL_ZIP" <<'PY'
import hashlib
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

out, commit, artifact_name = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
h = hashlib.sha256()
with artifact_name.open("rb") as fh:
    for chunk in iter(lambda: fh.read(1024 * 1024), b""):
        h.update(chunk)
out.write_text(json.dumps({
    "schema": 1,
    "platform": "macOS",
    "git_commit": commit,
    "generated_at": datetime.now(timezone.utc).isoformat(),
    "artifact": artifact_name.name,
    "sha256": h.hexdigest(),
    "signed": True,
    "notarized": True,
    "stapled": True,
}, indent=2, sort_keys=True) + "\n")
PY

echo "Verified notarized artifact: $FINAL_ZIP"
