# Edge Service Stack Feature Lifecycle

Source: `/Users/jpteasdale/plugins/edge-service-stack/skills/feature-development-lifecycle/SKILL.md`

Use this for day-to-day product work in an existing app.

## Working Rule

Prefer generated contracts at every boundary:

- Drizzle schema generates SQL migrations.
- Better Auth CLI generates auth schema changes.
- Hono OpenAPI generates TypeScript API paths.
- TanStack Router generates route tree types.
- Wrangler generates Worker binding types.
- shadcn CLI generates reusable primitives.

Hand-edit product logic, not generated contracts.

The generated API path contract is `src/lib/api/v1.d.ts`. Server workflows use the domain and repository model from [[stacks/edge-service-stack/directory-structure]]; this stack does not use TanStack server functions.

## Sequence

1. Write or extract a tiny feature brief: user story, workflow, data, access rules, API/UI routes, analytics, deployment concerns.
2. Read the current shape with `rg`, `rg --files`, and the repo's check command.
3. Update the data model, RLS, repositories, and domain workflow before API/UI when persistence is involved.
4. Add typed Hono OpenAPI routes and run `pnpm run gen:api`.
5. Build the TanStack UI from the generated API/auth/router/schema contracts.
6. Verify auth at UI, API middleware, and RLS layers.
7. Add typed analytics, audit records, and rate limits where risk warrants them.
8. Match tests to risk: RLS/API/component/Playwright/regression.
9. Run relevant generators and checks.
10. Leave deployment notes for migrations, bindings, secrets, analytics, and rollback risk.

## Mandatory Questions

- Is this following current official-source best practice for each touched tool?
- Is this duplicated, or should it move into a shared location?
- Are these types already declared somewhere we can import them from?
- Can frontend types come from generated API/auth/router/schema outputs?
- Did we regenerate every contract affected by this change?
- Are auth, RLS, and route guards enforcing the same access rule at the right layers?
- Are loading, empty, error, success, and permission-denied states covered?
- Is there a project-wide lint/audit command that should catch this class of mistake?

## Definition Of Done

- Data model and RLS are correct.
- API contract is generated and consumed by the UI.
- Auth is enforced server-side.
- UI covers loading, empty, error, and success states.
- Analytics/audit/rate limits are present where needed.
- Tests and build pass.
- Deployment notes are clear.
