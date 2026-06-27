#!/usr/bin/env bash
# Internal: the actual training + export + install pipeline.
# Spawned by tools/auto.sh — do not run directly.
#
# Cross-platform: detects macOS vs Linux and uses the right notifier + build.
#   macOS : osascript notification, xcodegen + xcodebuild rebuild
#   Linux : notify-send notification, ONNX is installed for the ort runtime
#           (no app rebuild here — `cargo tauri build` is heavy and optional)
#
# Args: REPO_PATH DATA_YAML [extra train_logos.py args...]
set -uo pipefail

REPO="$1"; shift
DATA="$1"; shift

cd "$REPO"

OS="$(uname -s)"   # Darwin | Linux

note() { echo "[$(date +%H:%M:%S)] $*"; }

notify() {
    local title="$1"
    local body="$2"
    local sound="${3:-Glass}"
    case "$OS" in
        Darwin)
            osascript -e "display notification \"${body//\"/\\\"}\" with title \"${title//\"/\\\"}\" sound name \"${sound}\"" >/dev/null 2>&1 || true
            ;;
        Linux)
            # notify-send if a notification daemon is around; otherwise no-op.
            command -v notify-send >/dev/null 2>&1 && notify-send "$title" "$body" >/dev/null 2>&1 || true
            ;;
        *) : ;;
    esac
}

cleanup() {
    rm -f tools/runs/auto.pid 2>/dev/null || true
}
trap cleanup EXIT

note "auto runner started ($OS)"
note "  Data: $DATA"
note "  Repo: $REPO"

# 1. Activate venv (created by setup_env.sh / setup_env.ps1).
note "Activating tools/.venv"
if [[ -f tools/.venv/bin/activate ]]; then
    # shellcheck source=/dev/null
    source tools/.venv/bin/activate
else
    note "FAILED: tools/.venv not found. Run tools/setup_env.sh first."
    notify "LiveBlock — venv missing" "Run tools/setup_env.sh first." "Sosumi"
    exit 1
fi

# 2. Train + export (+ install). train_logos.py exports CoreML on macOS and ONNX
#    everywhere; --install drops the model where the native app loads it.
note "Running train_logos.py --install"
if ! python tools/train_logos.py --data "$DATA" --install "$@"; then
    note "FAILED: training"
    notify "LiveBlock — training failed" "See tools/runs/auto.log" "Sosumi"
    exit 1
fi
note "Training + install OK"

# 3. Platform-specific app refresh.
case "$OS" in
    Darwin)
        note "Running xcodegen generate"
        if ! xcodegen generate >/dev/null 2>&1; then
            note "FAILED: xcodegen"
            notify "LiveBlock — xcodegen failed" "Training succeeded; Xcode project regen failed. See tools/runs/auto.log." "Sosumi"
            exit 2
        fi
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
        note "All steps OK. Trained model is installed and the macOS app is rebuilt."
        notify "LiveBlock — model ready" "Trained model installed and app rebuilt. Launch with ./run.sh."
        ;;
    Linux)
        # The ONNX export was already installed into the ort runtime dir + the
        # Linux Tauri resources by train_logos.py --install. A running app reads
        # the runtime model dir on next inference; no rebuild needed. We skip the
        # heavy `cargo tauri build` here — the user can rebuild when they want a
        # bundled release.
        note "All steps OK. Trained ONNX model installed for the Linux (ort) app."
        notify "LiveBlock — model ready" "Trained ONNX model installed. Relaunch LiveBlock to use it."
        ;;
    *)
        note "All steps OK (unknown OS '$OS'; skipped app refresh)."
        ;;
esac

exit 0
