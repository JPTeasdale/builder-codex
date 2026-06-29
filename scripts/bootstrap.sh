#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SKILL_SRC="$ROOT/skills/builder-dev-loop"
SKILL_DEST="${CODEX_HOME:-$HOME/.codex}/skills/builder-dev-loop"

mkdir -p "$(dirname "$SKILL_DEST")"

if [[ -L "$SKILL_DEST" || ! -e "$SKILL_DEST" ]]; then
  ln -sfn "$SKILL_SRC" "$SKILL_DEST"
else
  echo "Refusing to replace non-symlink skill path: $SKILL_DEST" >&2
  exit 1
fi

chmod +x "$SKILL_SRC"/scripts/*.py

echo "Linked $SKILL_DEST -> $SKILL_SRC"
echo "Open $ROOT/knowledge in Obsidian."
