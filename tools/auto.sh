#!/usr/bin/env bash
# Hands-off training: sleep-prevented, auto-export/install, native notification.
# CROSS-PLATFORM: works on macOS (caffeinate + Xcode rebuild) and Linux
# (systemd-inhibit + ONNX install). The actual workhorse is
# tools/_unattended_runner.sh, which is itself OS-aware.
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
#   2. macOS: trains under `caffeinate -i` (no idle-sleep). Linux: under
#      `systemd-inhibit` when available.
#   3. Auto-installs the trained model (.mlpackage on macOS, .onnx everywhere)
#   4. macOS: regenerates the Xcode project + builds. Linux: model is hot-loaded
#      by the running ort app; no rebuild required.
#   5. Sends a native notification on success or failure
#
# Foreground vs background:
#   - On macOS (interactive), the job self-backgrounds under caffeinate so you
#     can close the terminal.
#   - On Linux, the native app spawns this and reads our stdout live, so we run
#     the pipeline in the FOREGROUND. Set LB_FOREGROUND=1 to force this anywhere.
#
# Notes:
#   - macOS: keep your laptop lid OPEN. caffeinate prevents idle-sleep, not lid-sleep.
#   - Best on AC power (training is GPU-heavy).
set -euo pipefail

cd "$(dirname "$0")/.."
REPO="$(pwd)"
OS="$(uname -s)"   # Darwin | Linux
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
            # `stat` flags differ (BSD/macOS vs GNU/Linux); try both, ignore failure.
            started="$(stat -f %SB "$PIDFILE" 2>/dev/null || stat -c %y "$PIDFILE" 2>/dev/null || true)"
            [[ -n "$started" ]] && echo "  Started: $started"
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

# Pick a sleep-inhibitor wrapper appropriate to the OS. Empty array == no wrapper.
INHIBIT=()
case "$OS" in
    Darwin)
        # caffeinate -i prevents idle-sleep while training.
        INHIBIT=(caffeinate -i)
        ;;
    Linux)
        # systemd-inhibit keeps the box awake on most desktops; optional.
        if command -v systemd-inhibit >/dev/null 2>&1; then
            INHIBIT=(systemd-inhibit --what=idle:sleep --why="LiveBlock training" --mode=block)
        fi
        ;;
esac

# Decide foreground vs background.
#   - macOS interactive: self-background under caffeinate so the user can close
#     the terminal.
#   - Linux (native app spawns us and reads stdout) OR LB_FOREGROUND=1: run in
#     the foreground so output streams to the caller.
RUN_FOREGROUND=0
if [[ -n "${LB_FOREGROUND:-}" ]]; then
    RUN_FOREGROUND=1
elif [[ "$OS" == "Linux" ]]; then
    RUN_FOREGROUND=1
fi

if [[ "$RUN_FOREGROUND" -eq 1 ]]; then
    echo "$$" > "$PIDFILE"
    # tee so the live log file is populated while we also stream to our stdout
    # (the native app captures our stdout/stderr directly).
    # ${INHIBIT[@]+...} guard: safe expansion of a possibly-empty array under set -u.
    ${INHIBIT[@]+"${INHIBIT[@]}"} bash "$REPO/tools/_unattended_runner.sh" "$REPO" "$DATA" "$@" 2>&1 \
        | tee "$LOG"
    rc="${PIPESTATUS[0]}"
    rm -f "$PIDFILE" 2>/dev/null || true
    exit "$rc"
fi

# macOS background path.
nohup ${INHIBIT[@]+"${INHIBIT[@]}"} bash "$REPO/tools/_unattended_runner.sh" "$REPO" "$DATA" "$@" \
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
keeps running. A native notification fires on success or failure.

Realistic timing: 30 min – 3 hrs depending on dataset size and machine.
EOF
