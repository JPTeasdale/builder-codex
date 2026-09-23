# Edge Service Stack Database

Source: `/Users/jpteasdale/code/builder-codex/tools/ultralint`

Use Neon Postgres with Drizzle ORM, postgres-js, generated migrations, and role-scoped transactions. Treat RLS as an application boundary. See [[stacks/edge-service-stack/directory-structure]] for the canonical schema, repository, and domain paths.

## Repository Model

- Drizzle schema is server-owned. Do not create `src/lib/db/schema` or leak inferred Drizzle row types into shared/browser contracts.
- Raw Drizzle, postgres-js, and Neon imports stay in the server database layer or schema-generation tooling.
- `src/server/db/roles.ts` directly exports `withUserRole`, `withServerRole`, `UserTransaction`, `ServerTransaction`, and `RoleTransaction`.
- Raw `db.*`, `context.db.*`, and `tx.*` queries live in `src/server/db/repositories`; exported repository functions receive a role-scoped transaction instead of `AppDb` or `ServerContext`.
- Repositories are persistence adapters. Runtime imports stay inside `src/server/db` plus database packages; shared contracts are type-only. Repositories do not import API routes, domain workflows, auth, request context, components, or external service clients.
- Business orchestration lives in `src/server/domain`; domain workflows call repositories and return explicit domain/API DTOs.
- API handlers call domain workflows rather than embedding queries or persistence business logic.

Use the enforced `keyId()` identity helper and `timestampOptimized()` timestamp helper for handwritten tables. Generated schema files are exempt, but handwritten schema does not declare direct ID columns or call `timestamp(...)` outside the shared helpers.

## Connection Rules

- Use postgres-js with `max: 1` for Worker request scope.
- Prefer Hyperdrive binding connection strings in Workers.
- Use `DATABASE_URL` for local scripts, Drizzle Kit, and owner-level initialization or migration tasks.
- Never set a role globally on a connection. Use `SET LOCAL ROLE` inside transactions.
- Keep role-aware transaction setup in `src/server/db/roles.ts`, above repositories. A repository that supports either caller accepts `RoleTransaction`; narrower operations accept `UserTransaction` or `ServerTransaction`.

## RLS Roles

Use a project prefix for login and data roles. Keep an authenticated or domain-specific user role, a system role for internal work, and optional organization/member roles when RLS depends on organization context. Every non-exempt table enables RLS and defines explicit policies.

## Migration Workflow

1. Edit server-owned schema files.
2. Run `pnpm run db:generate`.
3. Review generated SQL.
4. Run `pnpm run db:check`.
5. Run `pnpm run db:migrate` against the correct Neon branch.
6. Run `pnpm run db:init` after role or grant changes.

Prefer Neon branches for feature work. Add focused tests for RLS, role-scoped repository behavior, migrations, and soft-delete filters.
