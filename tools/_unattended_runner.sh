#!/usr/bin/env bash
# Internal: the actual training + build pipeline.
# Spawned by tools/auto.sh — do not run directly.
#
# Args: REPO_PATH DATA_YAML [extra train_logos.py args...]
set -uo pipefail

REPO="$1"; shift
DATA="$1"; shift

cd "$REPO"

note() { echo "[$(date +%H:%M:%S)] $*"; }

notify() {
    local title="$1"
    local body="$2"
    local sound="${3:-Glass}"
    osascript -e "display notification \"${body//\"/\\\"}\" with title \"${title//\"/\\\"}\" sound name \"${sound}\"" >/dev/null 2>&1 || true
}

cleanup() {
    rm -f tools/runs/auto.pid 2>/dev/null || true
}
trap cleanup EXIT

note "auto.sh started"
note "  Data: $DATA"
note "  Repo: $REPO"

# 1. Train + install .mlpackage
note "Activating tools/.venv"
# shellcheck source=/dev/null
source tools/.venv/bin/activate

note "Running train_logos.py --install"
if ! python tools/train_logos.py --data "$DATA" --install "$@"; then
    note "FAILED: training"
    notify "LiveBlock — training failed" "See tools/runs/auto.log" "Sosumi"
    exit 1
fi
note "Training + install OK"

# 2. Regenerate Xcode project (in case anything changed)
note "Running xcodegen generate"
if ! xcodegen generate >/dev/null 2>&1; then
    note "FAILED: xcodegen"
    notify "LiveBlock — xcodegen failed" "Training succeeded; Xcode project regen failed. See tools/runs/auto.log." "Sosumi"
    exit 2
fi

# 3. Build
note "Running xcodebuild"
if ! xcodebuild \
       -project LiveBlock.xcodeproj \
       -scheme LiveBlock \
       -destination 'platform=macOS' \
       -configuration Debug \
       build >/dev/null 2>&1; then
    note "FAILED: xcodebuild"
    notify "LiveBlock — build failed" "Training succeeded but the app failed to build. See tools/runs/auto.log." "Sosumi"
    exit 3
fi

note "All steps OK. Trained model is installed and the app is rebuilt."
notify "LiveBlock — model ready" "Trained model installed and app rebuilt. Launch with ./run.sh."
exit 0
