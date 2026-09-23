# Edge Service Stack Deployment

Source: `/Users/jpteasdale/plugins/edge-service-stack/skills/reference-deployment/SKILL.md`

Deploy as a Cloudflare Worker running TanStack Start SSR plus Hono APIs. Reach Neon through Hyperdrive. Use Wrangler as the deployment source of truth.

## Wrangler Shape

`wrangler.jsonc` should include:

- stable Worker `name`
- `main: "src/server.ts"`
- current `compatibility_date`
- `compatibility_flags: ["nodejs_compat"]`
- Cloudflare Vite static assets shape when applicable
- observability enabled for production unless intentionally disabled
- `vars.APP_ENV` and `vars.APP_URL`
- Hyperdrive binding for Neon
- KV namespace for Better Auth secondary storage and app state
- optional R2, AI, Durable Objects, migrations
- `env.preview` with separate resources/placeholders

Secrets go through Wrangler secrets, not `vars`.

## Neon And Hyperdrive

Production flow:

1. Create Neon project/database.
2. Run migrations from owner `DATABASE_URL`.
3. Create app login and data roles with `pnpm run db:init`.
4. Create Hyperdrive using app login role connection string.
5. Put Hyperdrive ID in `wrangler.jsonc`.
6. Run `pnpm run gen:cf`.

Preview flow:

1. Create Neon branch.
2. Create preview Hyperdrive binding.
3. Replace preview placeholders before deploy.
4. Run migrations and `pnpm run db:init` against the branch.
5. Set preview secrets.
6. Run `pnpm run gen:cf`.

## Pre-Deploy

Run relevant commands:

- `pnpm run gen:cf`
- `pnpm run gen:auth`
- `pnpm run db:generate`
- `pnpm run db:check`
- `pnpm run gen:api` after route changes
- `pnpm run check`
- `pnpm run lint`
- `pnpm run test`
- `pnpm run ultralint`
- `pnpm run build`

Deploy with `pnpm run build && pnpm exec wrangler deploy` or an explicit preview environment when applicable.
