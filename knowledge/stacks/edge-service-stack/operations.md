# Edge Service Stack Operations

Source: `/Users/jpteasdale/plugins/edge-service-stack/skills/reference-operations/SKILL.md`

Operations combine Cloudflare observability, PostHog analytics, typed app events, health routes, rate limits, preview checks, and runbooks.

## Operational CLIs

Use:

- `pnpm run check`
- `pnpm run lint`
- `pnpm run test`
- `pnpm run test:e2e`
- `pnpm run gen:api`
- `pnpm run gen:cf`
- `pnpm run ultralint`
- `pnpm exec wrangler tail`

Write custom operational scripts only when an existing CLI cannot express the repeatable workflow.

## Observability Layers

- Cloudflare Worker logs, traces, invocation logs, and head sampling.
- Hono request logging with request IDs.
- API error handler that avoids leaking secrets.
- PostHog product events, funnels, feature behavior, and client-side errors.
- Public health endpoint.
- Audit tables for sensitive product actions.

## Analytics

Prefer first-party PostHog ingestion:

1. Browser initializes PostHog with `api_host: "/ingest"`.
2. `src/server.ts` proxies `/ingest` to PostHog.
3. UI calls typed event helpers.

Event properties should be small, explicit, and free of secrets, raw PHI/PII, full prompts, auth tokens, or unredacted provider payloads.

## Production Checklist

- Static checks, tests, and E2E pass.
- API and Worker generated types are current.
- Migrations reviewed and applied.
- RLS validation tests pass.
- Preview deploy tested with real auth callback URL.
- PostHog event appears.
- Cloudflare logs/traces visible.
- Rate-limited endpoints return 429 under abuse.
- Rollback path documented.
