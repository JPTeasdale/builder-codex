#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from common import agent_dir, emit, write_json


def main() -> int:
    parser = argparse.ArgumentParser(description="Create a manual ticket artifact for Builder Codex.")
    parser.add_argument("--worktree", required=True)
    parser.add_argument("--ticket-id", default="MANUAL-001")
    parser.add_argument("--title", required=True)
    parser.add_argument("--body", required=True)
    parser.add_argument("--label", action="append", default=[])
    args = parser.parse_args()

    worktree = Path(args.worktree).expanduser().resolve()
    ticket = {
        "id": args.ticket_id,
        "title": args.title,
        "body": args.body,
        "repo": str(worktree),
        "labels": args.label,
        "links": [],
        "source": "manual",
    }
    path = agent_dir(worktree) / "ticket.json"
    write_json(path, ticket)
    emit({"status": "ok", "next_action": "plan", "ticket": ticket, "artifacts": [str(path)]})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
