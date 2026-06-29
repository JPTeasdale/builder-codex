---
name: builder-dev-loop
description: Execute a structured software development workflow from a ticket or requested change through planning, implementation, deterministic checks, agentic quality evaluation, preview artifacts, human approval, and deploy gating. Use when Codex is asked to work a Linear/GitHub/product ticket, run a repeatable ticket-to-deploy loop, coordinate with the builder-codex scripts, consult the builder knowledge vault, or prepare implementation artifacts for approval.
---

# Builder Dev Loop

## Operating Model

Use Codex for judgment-heavy work and the bundled scripts for repeatable operations.

- Read the workflow reference first: `references/workflow.md`.
- Read the prompt/script contract when using or editing scripts: `references/prompt-contracts.md`.
- Treat `/Users/jpteasdale/code/builder-codex/knowledge/index.md` as the entry point for development knowledge.
- Load only the knowledge files relevant to the ticket, repo, labels, or changed area.
- Write per-ticket artifacts under `.agent/` in the active worktree.
- Prefer structured JSON script output over prose when handing state between scripts and Codex.

## Standard Loop

1. Establish the ticket or requested change.
2. Run `scripts/next_ticket.py` if the user did not provide a ticket.
3. Read `.agent/ticket.json` when present.
4. Read the knowledge vault index and any relevant files it routes to.
5. Create or confirm a focused plan in `.agent/plan.md`.
6. Run `scripts/create_worktree.py` when a new worktree is needed.
7. Implement the change in the worktree using existing repo conventions.
8. Run deterministic checks with `scripts/run_checks.py`.
9. Start or record preview details with `scripts/start_preview.py` when the change has a UI.
10. Collect artifacts with `scripts/collect_artifacts.py`.
11. If deployment or irreversible action is requested, run `scripts/request_telegram_approval.py` or ask the user directly.
12. Deploy only after explicit approval, then write `.agent/final-report.md`.

## Approval Rules

- Never deploy, merge, delete production data, or run destructive operations without explicit approval.
- Telegram approval is acceptable only when the approval script returns `approved: true` for the current ticket and action.
- If Telegram is not configured, stop at the approval gate and report the preview/check artifacts.
- Treat any ambiguous approval, stale approval, or mismatched ticket id as rejected.

## Knowledge Vault

The source-controlled knowledge vault is outside the skill so it can be edited in Obsidian:

`/Users/jpteasdale/code/builder-codex/knowledge`

Start with `knowledge/index.md`. Follow its links to repo-specific, product, frontend, backend, testing, and evaluation guidance. Do not ingest the whole vault by default.

## Script Contract

All scripts should:

- Accept `--worktree` when they operate on code.
- Accept `--ticket` or `--ticket-file` when ticket context matters.
- Print a single JSON object to stdout.
- Write durable artifacts under `.agent/`.
- Return non-zero only for infrastructure/script failure. Product checks can fail while still returning JSON with `status: "failed"`.

See `references/prompt-contracts.md` for canonical JSON shapes.
