# Merge And Deploy Authority

- The user merges pull requests only through their authorized GitHub account.
- Telegram messages are notification-only. They contain no Approve/Deny controls and never authorize a merge or deployment.
- A plain PR-ready notification includes both the GitHub PR URL and the verified preview URL.
- After notification, `devflow-tools wait-for-github --url <pr-url>` may observe merge or closure. Its registration and terminal message grant no authority.
- Exact Actions runs may be observed with `devflow-tools wait-for-github`; use `--deployment-url` only when endpoint health is a required success condition.
- A separate non-Telegram deploy authorization, when required, must explicitly match the ticket, target environment, action, risks, and current revision.
