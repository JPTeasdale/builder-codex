---
name: development-flow
description: Execute a minimal-change software development workflow from ticket intake through implementation, deterministic checks, independent code review, pull-request handoff, and verified release. Use when Codex is asked to implement or review a feature, bug fix, refactor, development ticket, pull request, or release.
---

# Development Flow

## Operating model

- Read `references/workflow.md` before changing code.
- Read `references/code-review.md` before delegating or performing the review gate.
- Prefer `~/devflow-tools/bin/devflow-tools` whenever it supports the needed queue, secret, or GitHub observation operation.
- Inspect the CLI's current help and relevant subcommand help instead of relying on command syntax copied into this skill.
- Use Codex judgment for planning, implementation, review, and release verification; use deterministic tools for repeatable state transitions and checks.
- Preserve unrelated user changes and keep ticket artifacts under `.agent/` in the active worktree.

## Core workflow

1. Establish one bounded current task. When none was supplied, prefer the Devflow task queue before consulting another ticket source.
2. Read the ticket, repository instructions, relevant knowledge, and surrounding code.
3. Define acceptance criteria and write the smallest credible implementation plan.
4. Use an isolated worktree when edits should not share the current checkout.
5. Implement the smallest coherent change that satisfies the acceptance criteria.
6. Use Devflow for a required secret handoff instead of asking for or carrying a secret in chat.
7. Run the narrowest relevant deterministic checks, followed by the repository's required full checks.
8. Run independent correctness and minimality reviews. Address findings with the smallest safe fix, rerun affected checks, and re-review changed areas.
9. Verify user-visible behavior and remote previews when relevant.
10. Commit and push a focused branch, create or update the pull request, and record exact revision and verification artifacts.
11. Notify the user through the configured channel. Telegram is notification-only; the user merges through GitHub.
12. Prefer Devflow for durable PR, CI, deployment, and release observation. Independently verify the authoritative terminal result.
13. Keep failures in the current task, prepare the smallest repair, and repeat the review and handoff gates.
14. Mark the task complete only after release verification, then select the next queued task without overlapping implementations.

## Change discipline

- Change only what the acceptance criteria require.
- Prefer existing repository patterns and dependencies over new abstractions, wrappers, helpers, or configuration.
- Add an abstraction only when it removes demonstrated duplication or enforces a necessary boundary in the current change.
- Avoid opportunistic cleanup, broad renames, formatting churn, generated-file edits without their generator, and unrelated dependency updates.
- Keep APIs and persisted state backward compatible unless the task explicitly requires a break and the user authorizes the migration.
- Tests should prove the requested behavior and important failure modes without overspecifying implementation details.

## Review gate

- When review agents are available, give them the raw ticket, complete diff, relevant surrounding code, and check results—not the intended answer or suspected defects.
- Run a defect-first correctness review and a separate minimality/cleanliness review as described in `references/code-review.md`.
- Require findings to be specific, evidenced, introduced by the change, and worth fixing. Ignore speculative concerns and style preferences.
- Do not proceed to pull-request handoff with unresolved correctness, security, data-loss, or release-blocking findings.

## Safety and authority

- Never merge on the user's behalf. The user merges through their authorized GitHub account.
- Telegram messages never authorize a merge or deployment and contain no approval controls.
- Never expose secrets in prompts, task artifacts, logs, command arguments, or notifications.
- Treat asynchronous watch registration as observation setup, not proof of success.
- Require explicit authorization for destructive actions or deployments not already authorized by the repository's normal merge-triggered release path.
- Do not overlap implementations; planning or reprioritizing future work is allowed.

## Knowledge

Start with `/Users/jpteasdale/code/builder-codex/knowledge/index.md`, then load only the notes relevant to the repository and changed area. Prefer current repository behavior over stale knowledge notes.
