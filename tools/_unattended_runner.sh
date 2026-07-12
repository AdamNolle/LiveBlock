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

# 1. Train + export a candidate .mlpackage. Direct installation is forbidden:
# only tools/install_verified_model.py may replace a runtime/bundled model after
# a complete passing schema-5 report.
note "Activating tools/.venv"
# shellcheck source=/dev/null
source tools/.venv/bin/activate

note "Running train_logos.py (candidate-only; no installation)"
if ! python tools/train_logos.py --data "$DATA" "$@"; then
    note "FAILED: training"
    notify "LiveBlock — training failed" "See tools/runs/auto.log" "Sosumi"
    exit 1
fi
note "Training + candidate export OK"
note "Candidate was NOT installed. Run the schema-5 promotion gate, then tools/install_verified_model.py."
notify "LiveBlock — candidate ready" "Training finished. Verification is required before installation."
exit 0
