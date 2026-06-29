from __future__ import annotations

import json
from pathlib import Path
from typing import Any


def agent_dir(worktree: str | Path) -> Path:
    path = Path(worktree).expanduser().resolve() / ".agent"
    path.mkdir(parents=True, exist_ok=True)
    return path


def write_json(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data, indent=2))
