# Edge Service Stack Review Gates

Source: `/Users/jpteasdale/plugins/edge-service-stack/skills/project-analysis-review/SKILL.md`

Run this before implementation, after the first working pass, and before final delivery.

## Non-Negotiable Questions

1. Is this following best practices from the official source for each technology touched?
2. Is this code duplicated, or can it be refactored into a shared location and reused?
3. Are these types already declared somewhere we can import them from?

Frontend components should import generated or shared types whenever possible:

- Hono OpenAPI generated `paths` from `src/lib/api/v1.d.ts`.
- API client helpers from `src/lib/api/client.ts`.
- Better Auth session/user types exposed through the shared auth module.
- TanStack Router route types and params.
- Zod schemas from API or shared validation modules.

Local frontend types are acceptable for presentational component props. Domain records, API payloads, auth sessions, and route params should not be redefined in route or component files. Drizzle inferred row types remain inside the server schema, repository, and domain layers; convert them to explicit domain/API DTOs before they reach shared or browser code.

## Passes

- Official-source pass: consult current docs/MCPs for touched subsystems.
- Duplication pass: search before adding abstractions.
- Type contract pass: prefer generated/shared contracts and run `ultralint`.
- Architecture boundary pass: Worker/API/auth/RLS/UI/analytics boundaries remain intact.
- Generated artifact pass: rerun affected generators.
- UI state pass: loading, empty, error, success, permission-denied, mobile, text fit.
- Test and operations pass: focused tests, regression tests, secrets, bindings, migrations, analytics, rollback.

## Review Done

- Official-source best practices were checked.
- Duplicate logic was removed or intentionally kept local.
- Frontend imports generated/shared types instead of redefining contracts.
- Project audit has no errors.
- Relevant generated artifacts are current.
- Tests and build pass, or gaps are explicitly reported.
