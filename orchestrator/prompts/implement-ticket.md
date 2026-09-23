Use $builder-dev-loop to implement the ticket described in `.agent/ticket.json`.

Follow the skill workflow:

1. Read the ticket.
2. Read `/Users/jpteasdale/code/builder-codex/knowledge/index.md` and relevant linked notes.
3. Write or update `.agent/plan.md`.
4. Read the Devflow integration rules. Use `devflow-tools tasks get-next` to select queued work, `tasks add` only for newly queued future work, `tasks reorder` only for intentional priority changes, `request-secret` for missing local values, and `wait-for-github` for exact PR or Actions-run observation at the documented points. Record only safe receipts in `.agent/devflow.json`.
5. Implement the smallest coherent change.
6. Run deterministic checks.
7. Collect artifacts.
8. Send a plain PR-ready notification with both PR and preview links when configured, then let the user merge in GitHub. Telegram never supplies approval or merge authority.
9. Register required GitHub waits without treating registration as success; verify terminal results before completing the ticket.

Return a concise final report with changed files, checks run, Devflow receipts, artifacts, and any separate deploy authorization needed.
