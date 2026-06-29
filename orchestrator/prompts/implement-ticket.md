Use $builder-dev-loop to implement the ticket described in `.agent/ticket.json`.

Follow the skill workflow:

1. Read the ticket.
2. Read `/Users/jpteasdale/code/builder-codex/knowledge/index.md` and relevant linked notes.
3. Write or update `.agent/plan.md`.
4. Implement the smallest coherent change.
5. Run deterministic checks.
6. Collect artifacts.
7. Stop at deploy or merge approval gates unless explicit approval is already present in `.agent/approval.json`.

Return a concise final report with changed files, checks run, artifacts, and any approval needed.
