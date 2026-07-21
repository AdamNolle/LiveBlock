#!/usr/bin/env bash
# One-command launcher for attributable sports-ad corpus review.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PYTHON="tools/.venv/bin/python"
if [[ ! -x "$PYTHON" ]]; then
  echo "LiveBlock's local tools environment is missing." >&2
  echo "Run ./tools/setup_env.sh, then launch this reviewer again." >&2
  exit 1
fi

export PYTHONPATH=tools
exec "$PYTHON" tools/corpus/review_labels.py \
  --pool tools/datasets/sports-ads/pool \
  --plan-file tools/datasets/sports-ads/human-review-plan.json \
  --port "${LIVEBLOCK_REVIEW_PORT:-8765}" \
  "$@"
