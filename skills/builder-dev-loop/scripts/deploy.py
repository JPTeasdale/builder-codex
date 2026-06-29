#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shlex
import subprocess
from pathlib import Path

from common import agent_dir, emit


def main() -> int:
    parser = argparse.ArgumentParser(description="Run an approved deploy command.")
    parser.add_argument("--worktree", required=True)
    parser.add_argument("--ticket-id", required=True)
    parser.add_argument("--command", required=True)
    args = parser.parse_args()

    approval_path = agent_dir(args.worktree) / "approval.json"
    if not approval_path.exists():
        emit({"status": "blocked", "next_action": "approval", "message": "Missing approval.json"})
        return 0

    approval = json.loads(approval_path.read_text(encoding="utf-8"))
    if approval.get("ticket_id") != args.ticket_id or not approval.get("approved"):
        emit({"status": "blocked", "next_action": "approval", "message": "Approval is missing, rejected, or for a different ticket."})
        return 0

    completed = subprocess.run(shlex.split(args.command), cwd=Path(args.worktree).expanduser().resolve())
    emit({"status": "ok" if completed.returncode == 0 else "failed", "next_action": "report", "exit_code": completed.returncode})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
