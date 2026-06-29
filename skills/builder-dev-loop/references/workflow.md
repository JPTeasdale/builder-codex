# Builder Dev Loop Workflow

## Goal

Move one bounded software change from ticket intake to a deploy decision with durable artifacts and clear human approval.

## Phases

### 1. Intake

Use the supplied ticket/request. If none exists, run `scripts/next_ticket.py`. Normalize the result into `.agent/ticket.json`.

Minimum ticket shape:

```json
{
  "id": "ENG-123",
  "title": "Short title",
  "body": "Ticket body or request",
  "repo": "/absolute/path/to/repo",
  "labels": ["frontend"],
  "links": []
}
```

### 2. Context

Read `knowledge/index.md`, then only the relevant knowledge files. For a UI task, prefer frontend, accessibility, testing, and UX evaluation notes. For backend work, prefer API, data, and reliability notes. For a known repo, read `knowledge/repos/<repo>.md` if present.

### 3. Plan

Write `.agent/plan.md` with:

- ticket summary
- constraints and assumptions
- files or modules likely to change
- deterministic checks to run
- approval/deploy risk

### 4. Worktree

Use `scripts/create_worktree.py` when the ticket needs isolated edits. Reuse an existing ticket worktree when one already exists and is clean enough to continue.

### 5. Implementation

Make the smallest coherent change that satisfies the ticket. Follow repo conventions over generic preferences.

### 6. Deterministic Checks

Run `scripts/run_checks.py`. It detects common package managers and runs available checks. When it cannot infer commands, inspect the repo and run the most relevant commands manually.

### 7. Agentic Evaluation

For UI work, capture screenshots and inspect the rendered result. For backend or architecture work, run a review pass against the ticket, tests, failure modes, and deployment risk.

### 8. Approval

Prepare `.agent/final-report.md` and call `scripts/request_telegram_approval.py` for deploy or merge gates when Telegram is configured. Otherwise ask the user in the Codex thread.

### 9. Deploy

Run `scripts/deploy.py` only when the approval artifact matches the current ticket and target action.
