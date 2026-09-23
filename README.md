# builder-codex

A source-controlled development-agent system for running a repeatable ticket-to-release Codex workflow.

## Layout

```text
skills/                 Codex-discoverable skills, symlinked into ~/.codex/skills
knowledge/              Obsidian-friendly development knowledge vault
orchestrator/           CLI that calls Codex and coordinates state
scripts/                Repo-level bootstrap and validation scripts
templates/              Markdown templates for agent artifacts
tools/                  Repo-owned tools such as ultralint
```

## First Setup

```bash
cd /Users/jpteasdale/code/builder-codex
./scripts/bootstrap.sh
./scripts/validate.sh
```

Open `/Users/jpteasdale/code/builder-codex/knowledge` as an Obsidian vault.

## Run A Ticket Through Codex

```bash
python3 -m orchestrator.builder_codex.cli run \
  --repo /path/to/repo \
  --ticket-id MANUAL-001 \
  --title "Implement the requested change" \
  --body "The full task text"
```

The orchestrator writes ticket state under the target repo's `.agent/` directory and invokes `codex exec` with this repo attached as an additional directory.

## Devflow Tools

The development loop integrates with the installed local Devflow client at `~/devflow-tools/bin/devflow-tools`:

- `tasks get-next` selects the highest-priority pending work without claiming it.
- `tasks add` admits bounded future work after its acceptance criteria are defined.
- `tasks reorder` changes the global pending priority without starting work.
- `request-secret` sends a one-time task-bound form through Telegram without placing the value in Codex.
- `wait-for-github` durably observes exact pull requests, Actions runs, and optional deployment health endpoints.

Safe receipts are recorded in each worktree's `.agent/devflow.json`. A registration receipt is not proof of CI, deployment, or release success. Telegram is notification-only; the user merges through GitHub.

## Ultralint

The Rust `ultralint` source now lives at `tools/ultralint`.

```bash
./scripts/install-ultralint.sh
ultralint .
```
