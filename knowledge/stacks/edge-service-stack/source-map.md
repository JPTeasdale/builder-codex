# Edge Service Stack Source Map

Source: `/Users/jpteasdale/plugins/edge-service-stack/references/best-practices-sources.md`

Use official sources when a rule, API, or workflow may have changed. Prefer MCP/source tools when available; otherwise browse official docs only.

## Official Sources

- Neon: projects, branches, connection strings, migrations, RLS, database operations.
- Drizzle: schema, migrations, relations, RLS policies, `drizzle-zod`.
- Cloudflare: Workers, Wrangler, Hyperdrive, KV, R2, Durable Objects, AI, observability.
- Hono and `@hono/zod-openapi`: middleware, routes, validation, OpenAPI.
- TanStack Start/Router/Query: route trees, loaders, route context, SSR, query invalidation. Edge Stack policy does not use TanStack server functions.
- Better Auth: plugins, schema generation, adapters, cookies, trusted origins, Worker behavior.
- PostHog: first-party ingestion, event taxonomy, autocapture, person properties, privacy.
- OpenAI/AI SDKs: model behavior, streaming, tool calls, provider keys, safety-sensitive updates.

When official docs conflict with local stack policy, surface the conflict instead of silently choosing one.
