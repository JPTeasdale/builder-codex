#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path

from common import emit


def slugify(value: str) -> str:
    value = value.lower()
    value = re.sub(r"[^a-z0-9]+", "-", value)
    return value.strip("-")[:48] or "ticket"


def main() -> int:
    parser = argparse.ArgumentParser(description="Create a git worktree for a ticket.")
    parser.add_argument("--repo", required=True)
    parser.add_argument("--ticket-file")
    parser.add_argument("--ticket-id")
    parser.add_argument("--title", default="")
    parser.add_argument("--worktrees-dir")
    args = parser.parse_args()

    repo = Path(args.repo).expanduser().resolve()
    if args.ticket_file:
        ticket = json.loads(Path(args.ticket_file).read_text(encoding="utf-8"))
        ticket_id = ticket["id"]
        title = ticket.get("title", "")
    else:
        ticket_id = args.ticket_id or "MANUAL-001"
        title = args.title

    branch = f"builder/{ticket_id}-{slugify(title)}"
    base_dir = Path(args.worktrees_dir).expanduser().resolve() if args.worktrees_dir else repo.parent / "_worktrees"
    worktree = base_dir / f"{ticket_id}-{slugify(title)}"
    worktree.parent.mkdir(parents=True, exist_ok=True)

    if worktree.exists():
        emit({"status": "ok", "next_action": "implement", "worktree": str(worktree), "branch": branch, "message": "Worktree already exists."})
        return 0

    subprocess.run(["git", "-C", str(repo), "worktree", "add", "-b", branch, str(worktree)], check=True)
    emit({"status": "ok", "next_action": "implement", "worktree": str(worktree), "branch": branch, "artifacts": []})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
