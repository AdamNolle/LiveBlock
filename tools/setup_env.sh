#!/usr/bin/env bash
# Create a Python venv at tools/.venv and install training/export deps.
# coremltools + ultralytics require Python 3.10–3.12; many machines ship 3.13,
# so prefer a uv-managed 3.12 when available. Idempotent — safe to re-run.
set -euo pipefail

cd "$(dirname "$0")"

if [[ ! -d .venv ]]; then
    if command -v uv >/dev/null 2>&1; then
        echo "→ Provisioning Python 3.12 via uv…"
        uv python install 3.12
        uv venv --python 3.12 .venv
    else
        PY=${PYTHON:-python3}
        if ! command -v "$PY" >/dev/null; then
            echo "Python 3 not found. Install uv (brew install uv) or Python 3.12 (brew install python@3.12)." >&2
            exit 1
        fi
        PY_VER=$("$PY" -c 'import sys; print(f"{sys.version_info[0]}.{sys.version_info[1]}")')
        case "$PY_VER" in
            3.10|3.11|3.12) "$PY" -m venv .venv ;;
            *) echo "Python $PY_VER unsupported (need 3.10–3.12, coremltools breaks on 3.13)." >&2
               echo "Fix: 'brew install uv' (recommended) or 'PYTHON=\$(which python3.12) tools/setup_env.sh'." >&2
               exit 1 ;;
        esac
    fi
fi

echo "→ Installing requirements (this takes a few minutes — torch is large)…"
if command -v uv >/dev/null 2>&1; then
    uv pip install --python .venv/bin/python -r requirements.txt
else
    source .venv/bin/activate
    python -m pip install --upgrade pip wheel >/dev/null
    python -m pip install -r requirements.txt
fi

echo
echo "✓ Done. Verify:"
echo "    tools/.venv/bin/python --version            # expect 3.10–3.12"
echo "    tools/.venv/bin/python -c 'import ultralytics, coremltools; print(\"ok\")'"
