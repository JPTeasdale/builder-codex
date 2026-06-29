#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shlex
import subprocess
from pathlib import Path

from common import agent_dir, emit, write_json


def detect_commands(worktree: Path) -> list[list[str]]:
    package_json = worktree / "package.json"
    if package_json.exists():
        data = json.loads(package_json.read_text(encoding="utf-8"))
        scripts = data.get("scripts", {})
        package_manager = "npm"
        if (worktree / "pnpm-lock.yaml").exists():
            package_manager = "pnpm"
        elif (worktree / "yarn.lock").exists():
            package_manager = "yarn"

        commands: list[list[str]] = []
        for script in ("lint", "typecheck", "test", "build"):
            if script in scripts:
                commands.append([package_manager, "run", script])
        return commands

    if (worktree / "pyproject.toml").exists():
        commands = []
        if (worktree / "ruff.toml").exists() or (worktree / ".ruff.toml").exists():
            commands.append(["python3", "-m", "ruff", "check", "."])
        commands.append(["python3", "-m", "pytest"])
        return commands

    return []


def main() -> int:
    parser = argparse.ArgumentParser(description="Run detected deterministic checks.")
    parser.add_argument("--worktree", required=True)
    parser.add_argument("--command", action="append", help="Explicit command to run. May be repeated.")
    args = parser.parse_args()

    worktree = Path(args.worktree).expanduser().resolve()
    out_dir = agent_dir(worktree) / "checks"
    out_dir.mkdir(parents=True, exist_ok=True)
    commands = [shlex.split(command) for command in args.command] if args.command else detect_commands(worktree)
    results = []

    for command in commands:
        name = " ".join(command)
        log_path = out_dir / (name.replace("/", "-").replace(" ", "-") + ".log")
        completed = subprocess.run(command, cwd=worktree, capture_output=True, text=True)
        log_path.write_text((completed.stdout or "") + (completed.stderr or ""), encoding="utf-8")
        results.append(
            {
                "name": name,
                "status": "passed" if completed.returncode == 0 else "failed",
                "exit_code": completed.returncode,
                "log": str(log_path),
            }
        )

    status = "ok" if all(result["exit_code"] == 0 for result in results) else "failed"
    checks_path = agent_dir(worktree) / "checks.json"
    payload = {"status": status, "next_action": "evaluate", "checks": results, "artifacts": [str(checks_path)]}
    write_json(checks_path, payload)
    emit(payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
