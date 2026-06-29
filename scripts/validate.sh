#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PYTHON="$ROOT/.venv/bin/python"

if [[ ! -x "$PYTHON" ]]; then
  python3 -m venv "$ROOT/.venv"
  "$ROOT/.venv/bin/pip" install -r "$ROOT/requirements-dev.txt"
fi

"$PYTHON" "$HOME/.codex/skills/.system/skill-creator/scripts/quick_validate.py" "$ROOT/skills/builder-dev-loop"
"$PYTHON" -m orchestrator.builder_codex.cli doctor
"$PYTHON" -m orchestrator.builder_codex.cli run \
  --repo "$ROOT" \
  --ticket-id "DRY-RUN-001" \
  --title "Validate builder-codex wiring" \
  --body "Dry run only." \
  --dry-run >/tmp/builder-codex-dry-run.json

echo "Validation passed."
