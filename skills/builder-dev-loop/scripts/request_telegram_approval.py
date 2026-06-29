#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
from datetime import datetime, timezone

from common import agent_dir, emit, write_json


def main() -> int:
    parser = argparse.ArgumentParser(description="Create or request a deploy approval artifact.")
    parser.add_argument("--worktree", required=True)
    parser.add_argument("--ticket-id", required=True)
    parser.add_argument("--action", default="deploy")
    parser.add_argument("--message", required=True)
    parser.add_argument("--auto-reject", action="store_true")
    args = parser.parse_args()

    token = os.environ.get("TELEGRAM_BOT_TOKEN")
    chat_id = os.environ.get("TELEGRAM_CHAT_ID")
    approved = False
    approval_source = "not-configured"
    status = "blocked"
    next_action = "ask_user"

    if args.auto_reject:
        approval_source = "auto-reject"
    elif token and chat_id:
        approval_source = "telegram-configured-placeholder"
        status = "blocked"
        next_action = "await_telegram"

    approval = {
        "ticket_id": args.ticket_id,
        "action": args.action,
        "approved": approved,
        "approval_source": approval_source,
        "requested_at": datetime.now(timezone.utc).isoformat(),
        "message": args.message,
    }
    path = agent_dir(args.worktree) / "approval.json"
    write_json(path, approval)
    emit({"status": status, "next_action": next_action, "approved": approved, "approval": approval, "artifacts": [str(path)]})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
