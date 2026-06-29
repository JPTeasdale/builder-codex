#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from common import agent_dir, emit, write_json


def main() -> int:
    parser = argparse.ArgumentParser(description="Collect known .agent artifacts.")
    parser.add_argument("--worktree", required=True)
    args = parser.parse_args()

    root = agent_dir(args.worktree)
    names = ["ticket.json", "plan.md", "checks.json", "preview.json", "approval.json", "final-report.md"]
    artifacts = [str(root / name) for name in names if (root / name).exists()]
    screenshots = root / "screenshots"
    if screenshots.exists():
        artifacts.extend(str(path) for path in sorted(screenshots.iterdir()) if path.is_file())

    path = root / "artifacts.json"
    payload = {"status": "ok", "next_action": "approval", "artifacts": artifacts}
    write_json(path, payload)
    payload["artifacts"].append(str(path))
    emit(payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
