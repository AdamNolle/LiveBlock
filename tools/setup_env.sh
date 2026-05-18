#!/usr/bin/env bash
# Create a Python venv at tools/.venv and install training/export deps.
# Idempotent — safe to re-run.
set -euo pipefail

cd "$(dirname "$0")"

PY=${PYTHON:-python3}
if ! command -v "$PY" >/dev/null; then
    echo "Python 3 not found. Install Python 3.10+ (e.g. brew install python@3.11)." >&2
    exit 1
fi

PY_VER=$("$PY" -c 'import sys; print(f"{sys.version_info[0]}.{sys.version_info[1]}")')
case "$PY_VER" in
    3.10|3.11|3.12) ;;
    *) echo "Python $PY_VER detected. ultralytics + coremltools want 3.10–3.12. Override with PYTHON=path/to/python3.11 ./setup_env.sh" >&2 ;;
esac

if [[ ! -d .venv ]]; then
    echo "→ Creating venv at tools/.venv (using $PY)…"
    "$PY" -m venv .venv
fi

source .venv/bin/activate
echo "→ Upgrading pip…"
python -m pip install --upgrade pip wheel >/dev/null

echo "→ Installing requirements (this takes a couple minutes)…"
python -m pip install -r requirements.txt

echo
echo "✓ Done. Verify:"
echo "    source tools/.venv/bin/activate"
echo "    python -c 'import ultralytics, coremltools; print(\"ok\")'"
