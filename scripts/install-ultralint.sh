#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo install --locked --force --path "$ROOT/tools/ultralint"
ultralint --version

if [[ -f package.json ]]; then
  pnpm pkg set scripts.ultralint="ultralint ."
fi

echo "ultralint installed. Run: ultralint ."
