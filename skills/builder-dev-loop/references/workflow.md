# Development Workflow

## 1. Intake

Use the supplied request as the current task. If none was supplied, prefer the Devflow CLI's task-queue capabilities and inspect its current help to select the appropriate operation. Fall back to another configured ticket source only when no matching Devflow task exists.

Normalize the task into `.agent/ticket.json` with an identifier, title, bounded description, repository, relevant labels or links, and independently verifiable acceptance criteria. Queue newly discovered future work through Devflow when possible, but do not re-add the active task when a Codex task resumes.

## 2. Context

Read repository instructions and enough surrounding implementation, tests, and configuration to understand the current behavior. Read the development knowledge index and only the notes relevant to the repository or changed area.

Check the worktree before editing. Preserve unrelated changes and distinguish user-owned work from the current task.

## 3. Plan

Write `.agent/plan.md` with:

- the requested outcome and acceptance criteria;
- constraints and assumptions;
- the smallest likely file and module surface;
- deterministic checks and user-visible verification;
- migration, deployment, and rollback risk.

Reject plan steps that are merely cleanup, speculative extensibility, or unrelated refactoring.

## 4. Isolation

Use an isolated worktree when the task needs a separate branch or the current checkout contains overlapping work. Reuse an existing clean task worktree when appropriate.

## 5. Implementation

Implement the smallest coherent patch. Follow local conventions before introducing a new pattern. Reuse existing functions and boundaries when they remain clear; avoid wrappers that only rename another call or abstractions with one speculative consumer.

When a required local secret is missing, prefer the Devflow CLI's secure handoff capability. Let the CLI explain the correct operation. Never place the secret in a prompt, ticket, artifact, log, or command argument.

## 6. Deterministic checks

Run focused tests while iterating. Before review, run formatting, linting, type checks, tests, builds, and repository-specific structural checks that apply to the changed surface. Record commands, results, and useful logs in `.agent/checks.json`.

A queued or in-progress remote check is not success. When durable observation is useful, prefer the Devflow CLI and follow its current help, then independently inspect the authoritative result.

## 7. Code review

After deterministic checks, run the review gate from `references/code-review.md`.

Use independent review agents when available:

1. correctness reviewer;
2. minimality and cleanliness reviewer.

Give reviewers the raw ticket, complete diff, relevant surrounding code, and check results. Do not prime them with intended findings. Fix only evidenced, actionable issues and use the smallest safe patch. Rerun affected checks and re-review the changed areas.

## 8. Preview and evaluation

For user-interface work, inspect the rendered result at representative desktop and mobile sizes and verify accessibility-critical behavior. For backend or architecture work, exercise success, validation, authorization, persistence, restart, and failure paths proportionate to risk.

For a remote preview, verify that the exact deployed revision is reachable and implements the task. Record the preview URL and exact head/base revisions.

## 9. Pull request handoff

Commit and push only the focused task changes, then create or update the pull request. Record `.agent/pull-request.json` with the PR URL, number, branches, exact revisions, current checks, preview, risks, and rollback notes.

Do not call work ready before the required checks and preview are verified. Send the configured plain ready notification with the GitHub PR URL and verified preview URL when available. Telegram is notification-only; the user merges only through GitHub.

## 10. Merge and release observation

After the user merges, prefer Devflow for durable observation of the PR, CI, deployment, and release when supported. Let the CLI's current help select the correct operation. Treat notification as a wake-up signal and inspect the authoritative GitHub and deployment results.

Write `.agent/release.json` only after the exact released revision and target are verified. If a required job or verification fails, keep the current task active and prepare the smallest repair PR rather than bypassing the failure.

## 11. Completion and continuation

Update `.agent/final-report.md` with the outcome, focused diff, checks, review results, pull request, notification, release verification, residual risks, and follow-up work.

Complete the current task only after verified release. Then prefer the Devflow queue for the next task. Never overlap implementation work; planning and reprioritizing future tasks is allowed.
