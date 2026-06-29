# builder-codex

A source-controlled development-agent system for running a repeatable ticket-to-deploy Codex workflow.

## Layout

```text
skills/                 Codex-discoverable skills, symlinked into ~/.codex/skills
knowledge/              Obsidian-friendly development knowledge vault
orchestrator/           CLI that calls Codex and coordinates state
scripts/                Repo-level bootstrap and validation scripts
templates/              Markdown templates for agent artifacts
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
