# Edge Service Stack Auth

Source: `/Users/jpteasdale/code/builder-codex/tools/ultralint`

Use Better Auth with Drizzle, Worker environment bindings, KV secondary storage, Hono passthrough routes, and role-scoped database access. See [[stacks/edge-service-stack/directory-structure]] for canonical auth, API, repository, and shared-client paths.

## Worker Factory

- Keep Better Auth package imports inside `src/server/auth/better-auth.ts`.
- Export only request-scoped `createAuth(...)` and `type Auth = ReturnType<typeof createAuth>` from the factory.
- Pass the request database, Worker `env`, and `ExecutionContext` into the factory. Do not construct a module-level runtime singleton.
- Keep `better-auth.config.ts` as a CLI schema-generation shim that imports `createAuth`, passes typed placeholder Worker dependencies, and exports `auth` for the CLI. It must not recreate runtime options or re-export a runtime singleton.

Generate auth schema changes with:

```bash
pnpm run gen:auth
pnpm run db:generate
```

## Request Rules

- Mount Better Auth passthrough routes before protected route groups.
- Keep trusted origins exact per environment.
- Keep `BETTER_AUTH_SECRET` as a stable Worker secret for each environment.
- Prefix KV keys with app and environment identifiers when namespaces are shared.
- Keep browser-safe auth helpers behind the shared client/module boundary; they do not import server code.
- Do not add auth session TanStack server functions. Session and protected product operations flow through Hono middleware and OpenAPI endpoints.

## RLS Integration

Hono auth middleware should read the session from request headers, return `401` when it is missing, set the session on context, and establish the role-scoped database capability needed by repositories. Protected handlers call domain workflows and repositories instead of issuing raw database queries from middleware or route files.

## Permission Semantics

Define role-to-permission mapping and permission evaluation in one RBAC module under `src/server/auth` or `src/server/domain`. Permission strings use `<resource>:<action>:<scope>` with optional narrower scope segments, such as `documents:read:submitted:region`; resources are derived from non-underscore Drizzle table names. Routes, repositories, services, and components call named permission helpers instead of comparing values such as `region-admin` directly.
