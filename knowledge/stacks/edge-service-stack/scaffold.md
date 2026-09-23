# Edge Service Stack Scaffold

Source: `/Users/jpteasdale/plugins/edge-service-stack/skills/project-scaffold-starter/SKILL.md`

Use this for a greenfield service. Start with CLIs and generators, then patch in house architecture.

Use [[stacks/edge-service-stack/directory-structure]] for the canonical folder contract rather than restating it here.

## Product Inputs

Establish:

- app/package name
- Worker name and canonical app URL
- Neon project/database name
- database role prefix
- auth style
- storage, AI, Durable Objects, email, webhook needs

Default locally to `APP_ENV=development`, `APP_URL=http://localhost:3000`, one Hyperdrive binding named `DB`, and one KV namespace named `KV`.

## Target Starter Contract

The first scaffold should have:

- TanStack Start app served by Cloudflare Worker at `src/server.ts`.
- Hono OpenAPI routes under `/api`, with normal product endpoints versioned under `/api/v1`.
- Generated `src/lib/api/v1.d.ts` OpenAPI paths and a generated React Query client wrapper.
- Neon/Drizzle schema, migration, and role init scripts.
- Better Auth configured with generated Drizzle schema and `/api/auth/*` passthrough.
- shadcn-compatible primitives generated through CLI.
- PostHog wrapper and optional `/ingest` proxy.
- Wrangler config, Worker types, environment placeholders.
- Biome, Vitest, and Playwright installed and runnable.
- Docs for local setup, patterns, deployment, and secrets.

## Creation Sequence

1. Create a pnpm workspace and package metadata.
2. Install runtime and dev packages.
3. Add scripts for dev/build/deploy/check/lint/test/db/gen tasks.
4. Run generator CLIs: Biome, Playwright, shadcn, Wrangler.
5. Copy starter overlay from `/Users/jpteasdale/plugins/edge-service-stack/templates/starter-overlay`.
6. Replace app tokens and adapt generated files minimally.
7. Generate Worker/auth/db/API/router artifacts.
8. Install `ultralint`.
9. Run `pnpm run check`, `pnpm run lint`, `pnpm run test`, `pnpm run ultralint`, `pnpm run build`, and the dev server.

The scaffold is complete when future feature work can follow [[stacks/edge-service-stack/lifecycle]] without first inventing missing architecture.
