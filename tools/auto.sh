#!/usr/bin/env bash
# Hands-off training: backgrounded, sleep-prevented, auto-rebuild, macOS notification.
#
# Usage:
#   tools/auto.sh path/to/data.yaml                       # default training
#   tools/auto.sh path/to/data.yaml --epochs 30 --batch 8 # extra args forwarded to train_logos.py
#   tools/auto.sh status                                  # check on running job
#   tools/auto.sh log                                     # tail the log
#   tools/auto.sh stop                                    # cancel the running job
#
# What it does after you start it:
#   1. Sets up tools/.venv if missing
#   2. Trains under `caffeinate -i` so the Mac doesn't idle-sleep
#   3. Auto-installs the resulting .mlpackage into Sources/
#   4. Regenerates Xcode project + builds the app
#   5. Sends a macOS notification on success or failure
#
# After you start it, close this terminal — the job keeps running.
# Notes:
#   - Keep your laptop lid OPEN. caffeinate prevents idle-sleep, not lid-sleep.
#   - Best on AC power (training is GPU-heavy).
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$(pwd)"
LOG="$REPO/tools/runs/auto.log"
PIDFILE="$REPO/tools/runs/auto.pid"

mkdir -p "$REPO/tools/runs"

usage() {
    # Print the leading comment block only (everything until the first non-# line).
    awk 'NR==1{next} /^[^#]/{exit} {sub(/^# ?/,""); print}' "$0"
}

# Subcommands
case "${1:-}" in
    "")
        usage; exit 2 ;;
    -h|--help|help)
        usage; exit 0 ;;
    status)
        if [[ -f "$PIDFILE" ]] && pid=$(cat "$PIDFILE") && kill -0 "$pid" 2>/dev/null; then
            echo "✓ Running (PID $pid). Log: $LOG"
            echo "  Started: $(stat -f %SB "$PIDFILE")"
            echo "  Last lines:"
            tail -5 "$LOG" 2>/dev/null | sed 's/^/    /'
        else
            echo "✗ No active training job."
            [[ -f "$LOG" ]] && echo "  Last log: $(tail -1 "$LOG" 2>/dev/null)"
        fi
        exit 0 ;;
    log)
        [[ -f "$LOG" ]] || { echo "No log yet at $LOG"; exit 1; }
        exec tail -f "$LOG" ;;
    stop)
        if [[ -f "$PIDFILE" ]] && pid=$(cat "$PIDFILE") && kill -0 "$pid" 2>/dev/null; then
            kill "$pid" && echo "✓ Sent SIGTERM to $pid" || echo "Failed to kill $pid"
            # Children are caffeinate + bash + python; clean them too.
            pkill -P "$pid" 2>/dev/null || true
            rm -f "$PIDFILE"
        else
            echo "Nothing to stop."
        fi
        exit 0 ;;
esac

# Otherwise: $1 is the data.yaml, rest are extra args
DATA="$1"; shift
if [[ ! -f "$DATA" ]]; then
    echo "Dataset config not found: $DATA" >&2
    echo "Need a YOLO-format data.yaml. See tools/README.md → 'Get a dataset'." >&2
    exit 2
fi
DATA="$(cd "$(dirname "$DATA")" && pwd)/$(basename "$DATA")"  # absolute

if [[ -f "$PIDFILE" ]] && pid=$(cat "$PIDFILE") && kill -0 "$pid" 2>/dev/null; then
    echo "Another job is already running (PID $pid)."
    echo "Run 'tools/auto.sh stop' first, or 'tools/auto.sh status' to check."
    exit 3
fi

if [[ ! -d tools/.venv ]]; then
    echo "→ One-time: setting up tools/.venv (Python deps, ~3 min)…"
    bash tools/setup_env.sh
fi

# Background-launch the inner runner with caffeinate so idle-sleep is prevented.
nohup caffeinate -i bash "$REPO/tools/_unattended_runner.sh" "$REPO" "$DATA" "$@" \
    >"$LOG" 2>&1 &

PID=$!
echo "$PID" > "$PIDFILE"
disown "$PID" 2>/dev/null || true

cat <<EOF
✓ Training started in the background.
  PID:    $PID
  Data:   $DATA
  Log:    $LOG

Useful commands:
  tools/auto.sh log       # tail progress
  tools/auto.sh status    # one-line status check
  tools/auto.sh stop      # cancel

Mac stays awake via caffeinate. Close this terminal whenever — the job
keeps running. macOS notification fires on success or failure.

Realistic timing: 30 min – 3 hrs depending on dataset size and Mac model.
EOF
