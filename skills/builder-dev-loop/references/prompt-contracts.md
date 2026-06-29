# Prompt And Script Contracts

## Script JSON Envelope

Every script prints one JSON object:

```json
{
  "status": "ok",
  "next_action": "continue",
  "message": "Human-readable summary",
  "artifacts": ["/absolute/path/to/artifact"]
}
```

Use `status: "failed"` for completed checks that found product problems. Use a non-zero exit code for script crashes, missing dependencies, or invalid arguments.

## next_ticket.py

Output:

```json
{
  "status": "ok",
  "next_action": "plan",
  "ticket": {
    "id": "MANUAL-001",
    "title": "Implement requested change",
    "body": "Full request",
    "repo": "/absolute/path/to/repo",
    "labels": []
  },
  "artifacts": ["/absolute/path/.agent/ticket.json"]
}
```

## create_worktree.py

Output:

```json
{
  "status": "ok",
  "next_action": "implement",
  "worktree": "/absolute/path/to/worktree",
  "branch": "builder/ENG-123-slug",
  "artifacts": []
}
```

## run_checks.py

Output:

```json
{
  "status": "ok",
  "next_action": "evaluate",
  "checks": [
    {
      "name": "npm test",
      "status": "passed",
      "exit_code": 0,
      "log": ".agent/checks/npm-test.log"
    }
  ],
  "artifacts": ["/absolute/path/.agent/checks.json"]
}
```

## request_telegram_approval.py

Output:

```json
{
  "status": "ok",
  "next_action": "deploy",
  "approved": true,
  "approval": {
    "ticket_id": "ENG-123",
    "action": "deploy",
    "approved_by": "telegram:user",
    "approved_at": "2026-06-29T12:00:00Z"
  },
  "artifacts": ["/absolute/path/.agent/approval.json"]
}
```
