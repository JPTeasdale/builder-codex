# Code Review Agents

Run review after deterministic checks and before pull-request handoff. Use read-only review agents when available; otherwise perform the same passes locally.

## Shared review input

Provide each reviewer:

- the raw ticket and acceptance criteria;
- the complete diff against the correct merge base;
- relevant surrounding code and repository instructions;
- deterministic check results.

Do not provide the intended answer, suspected bug, or proposed fix. Review the entire diff. Report only discrete, actionable issues introduced by the change, with tight file and line references. Ignore pre-existing problems, speculative concerns, and style nits.

Review agents are read-only: do not modify files, create commits, post comments, or delegate. Present findings first in severity order, explain one concrete failing or excess scenario per finding, and then note material test gaps or residual risk. Return `No findings.` when there is no qualifying issue.

## Correctness reviewer

Review for behavior that violates the ticket or introduces a regression. Check boundary conditions, error handling, authorization, security, concurrency, persistence, migrations, restart behavior, compatibility, and missing high-value tests according to the changed surface.

Prioritize defects by impact. Explain the concrete failing scenario and why existing checks do not protect it. Return `No findings.` when there is no evidenced issue.

## Minimality and cleanliness reviewer

Review whether the patch is the smallest clear change that fully satisfies the ticket.

Flag only actionable excess or complexity, including:

- unrelated edits, formatting churn, or broad renames;
- new dependencies or configuration without demonstrated need;
- abstractions, wrappers, helpers, or indirection with no current payoff;
- duplicated logic that an existing local pattern already solves;
- dead compatibility paths or generated edits that should come from a generator;
- tests coupled to implementation details rather than behavior.

Do not request a rewrite merely because another design is possible. A simplification finding must identify exact code that can be removed or replaced while preserving acceptance criteria, readability, and safety. Return `No findings.` when the patch is already focused and clean.

## Author response

Address valid findings with the smallest safe edit. Do not broaden the ticket while fixing review feedback. Rerun affected deterministic checks, then ask reviewers to inspect only the revised diff and any directly affected call sites. Record the final review outcome in `.agent/final-report.md`.
