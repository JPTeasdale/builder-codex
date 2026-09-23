# Edge Service Stack API

Source: `/Users/jpteasdale/code/builder-codex/tools/ultralint`

Use Hono with `@hono/zod-openapi`. Every consumed route is typed, documented, versioned, and represented in generated OpenAPI types. See [[stacks/edge-service-stack/directory-structure]] for the canonical API, server, and client paths.

## Middleware Order

1. Error handler.
2. Request ID.
3. Rate limiting.
4. Request-scoped database and auth setup.
5. Request logging and audit middleware.
6. Public health route.
7. Better Auth passthrough.
8. Public feature routes.
9. Auth middleware for protected groups.
10. Role, admin, or ownership middleware.
11. Protected routes.
12. OpenAPI document endpoint.

## Route Contract

- Define consumed endpoints with `createRoute(...)` and mount them with `app.openapi(...)`.
- Declare request body, query, path parameters, and response schemas. Read validated input with `c.req.valid(...)`.
- Use `drizzle-zod` where table schemas map cleanly, but do not expose Drizzle row types as API contracts.
- Use `/api/v1/...` for normal app endpoints. Supported exceptions are `/api/auth/*`, `/api/health`, and `/api/v1/openapi.json`.
- Gate test-only routes to `APP_ENV === "test"` before registration or handling.
- Keep handlers transport-focused. Business workflows belong in the domain layer; raw database work belongs in repositories.
- Return explicit status codes and response bodies declared by the OpenAPI route.
- Register one centralized `app.onError(...)` boundary. Log original exceptions server-side, but return only stable public error codes and messages; never serialize raw errors, stacks, causes, or provider details.

Do not use manual pathname routers, ad hoc `request.json()` parsing, uncontracted `Response.json(...)`, or TanStack server functions as an alternate API layer.

## Generated Client Contract

After route changes, run:

```bash
pnpm run gen:api
```

Generation writes OpenAPI `paths` to `src/lib/api/v1.d.ts`. Browser code consumes those types through `src/lib/api/client.ts` and generated query or mutation helpers. UI code does not redefine response types or hand-write normal app API `fetch` calls.
