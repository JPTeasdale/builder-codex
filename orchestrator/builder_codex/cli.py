from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
PROMPT_PATH = REPO_ROOT / "orchestrator" / "prompts" / "implement-ticket.md"
KNOWLEDGE_ROOT = REPO_ROOT / "knowledge"


def _write_json(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def _read_prompt() -> str:
    return PROMPT_PATH.read_text(encoding="utf-8")


def cmd_run(args: argparse.Namespace) -> int:
    repo = Path(args.repo).expanduser().resolve()
    if not repo.exists():
        print(f"Repository does not exist: {repo}", file=sys.stderr)
        return 2

    agent_dir = repo / ".agent"
    ticket = {
        "id": args.ticket_id,
        "title": args.title,
        "body": args.body,
        "repo": str(repo),
        "labels": args.label,
        "links": args.link,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "source": "builder-codex-orchestrator",
    }
    _write_json(agent_dir / "ticket.json", ticket)

    codex_bin = os.environ.get("CODEX_BIN", "codex")
    command = [
        codex_bin,
        "exec",
        "--cd",
        str(repo),
        "--add-dir",
        str(REPO_ROOT),
        "--output-last-message",
        str(agent_dir / "codex-last-message.md"),
        _read_prompt(),
    ]
    if args.dry_run:
        print(json.dumps({"status": "ok", "command": command, "ticket": ticket}, indent=2))
        return 0

    completed = subprocess.run(command, cwd=repo)
    return completed.returncode


def cmd_doctor(args: argparse.Namespace) -> int:
    checks = {
        "repo_root": str(REPO_ROOT),
        "prompt_exists": PROMPT_PATH.exists(),
        "knowledge_index_exists": (KNOWLEDGE_ROOT / "index.md").exists(),
        "codex_available": subprocess.run(
            ["which", os.environ.get("CODEX_BIN", "codex")],
            capture_output=True,
            text=True,
        ).returncode
        == 0,
    }
    print(json.dumps(checks, indent=2))
    return 0 if all(checks.values()) else 1


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="builder-codex")
    subparsers = parser.add_subparsers(dest="command", required=True)

    run = subparsers.add_parser("run", help="Invoke Codex on a ticket")
    run.add_argument("--repo", required=True, help="Target repository/worktree")
    run.add_argument("--ticket-id", required=True, help="Ticket id, e.g. ENG-123")
    run.add_argument("--title", required=True, help="Ticket title")
    run.add_argument("--body", required=True, help="Ticket body or request")
    run.add_argument("--label", action="append", default=[], help="Ticket label")
    run.add_argument("--link", action="append", default=[], help="Related URL")
    run.add_argument("--dry-run", action="store_true", help="Print command without running Codex")
    run.set_defaults(func=cmd_run)

    doctor = subparsers.add_parser("doctor", help="Check local configuration")
    doctor.set_defaults(func=cmd_doctor)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
