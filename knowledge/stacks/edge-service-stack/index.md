# Edge Service Stack

Source plugin: `/Users/jpteasdale/plugins/edge-service-stack`

Use this stack knowledge when a repo uses Cloudflare Workers, TanStack Start, Hono OpenAPI, Neon/Drizzle, Better Auth, PostHog, Wrangler, and `ultralint`.

The executable architecture uses Hono OpenAPI under `src/server/api`, repository-backed persistence, generated `src/lib/api/v1.d.ts`, `.tsx` only under components, and no TanStack server functions. [[stacks/edge-service-stack/directory-structure]] is the sole folder-structure explanation.

## Workflows

- [[stacks/edge-service-stack/scaffold]] for new projects.
- [[stacks/edge-service-stack/lifecycle]] for feature work.
- [[stacks/edge-service-stack/review-gates]] before implementation, after first pass, and before delivery.
- [[stacks/edge-service-stack/ultralint]] for static structural enforcement.

## Contracts

- [[stacks/edge-service-stack/directory-structure]]
- [[stacks/edge-service-stack/secrets]]

## Subsystems

- [[stacks/edge-service-stack/api]]
- [[stacks/edge-service-stack/auth]]
- [[stacks/edge-service-stack/database]]
- [[stacks/edge-service-stack/ui]]
- [[stacks/edge-service-stack/deployment]]
- [[stacks/edge-service-stack/operations]]
- [[stacks/edge-service-stack/source-map]]

## Routing Rule

For ordinary tickets, start with lifecycle plus the subsystem notes touched by the change. Use scaffold only for greenfield projects. Use source-map when a rule depends on current third-party behavior.
