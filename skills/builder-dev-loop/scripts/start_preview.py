#!/usr/bin/env python3
from __future__ import annotations

import argparse

from common import agent_dir, emit, write_json


def main() -> int:
    parser = argparse.ArgumentParser(description="Record preview details for a worktree.")
    parser.add_argument("--worktree", required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--note", default="")
    args = parser.parse_args()

    path = agent_dir(args.worktree) / "preview.json"
    payload = {"status": "ok", "next_action": "collect_artifacts", "preview": {"url": args.url, "note": args.note}, "artifacts": [str(path)]}
    write_json(path, payload)
    emit(payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
