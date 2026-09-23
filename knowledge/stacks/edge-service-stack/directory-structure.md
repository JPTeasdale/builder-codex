# Edge Service Stack Directory Structure

Source: `/Users/jpteasdale/code/builder-codex/tools/ultralint`

This is the only Edge Stack knowledge page that defines the folder structure. Topic pages should link here and describe behavior, not copy the tree.

## App Slots

A single app lives at the repository root. It has root `package.json`, `tsconfig.json`, `vite.config.ts`, `wrangler.jsonc`, `drizzle.config.ts`, `better-auth.config.ts`, `docs/`, `scripts/`, and the source contract below. Root web and Worker policy both apply.

A multi-app repository uses `apps/*` for deployments, `packages/*` for shared packages, and root `docs/`, `infra/`, `package.json`, and `tsconfig.json`. Root `package.json` declares workspaces. `ultralint` detects:

- web apps from `vite.config.ts`, `src/routes`, or the router entrypoint; the canonical router file is `src/router.ts`
- Worker apps from `wrangler.jsonc`
- mobile apps from `capacitor.config.ts`
- shared packages from `packages/*/package.json`

Run `ultralint <project-root> --explain-config` to see the slots selected by the current executable.

## Web App Contract

The complete tree below is the combined single-app contract. In a multi-app repository, root `package.json`, `tsconfig.json`, `docs/`, and `infra/` stay at the repository root; each detected web app owns the shown app source and generator files; each detected Worker app owns `wrangler.jsonc` and its generated Worker types.

```text
package.json
tsconfig.json
vite.config.ts
wrangler.jsonc
drizzle.config.ts
better-auth.config.ts
worker-configuration.d.ts
scripts/
drizzle/
docs/
src/
  server.ts
  router.ts
  routeTree.gen.ts
  server/
    api/
      index.ts
      routes/
        <feature>.route.ts
      middleware/
    auth/
      better-auth.ts
    config/
      env.ts
      urls.ts
      features.ts
    cloudflare/
      bindings.ts
    clients/
    domain/
    db/
      index.ts
      roles.ts
      schema/
      repositories/
        <feature>.repository.ts
  lib/
    api/
      client.ts
      v1.d.ts
    -hooks/
    -schema/
    <module>/
      <module>.client.ts
      <module>.hooks.ts
      <module>.schema.ts
      <module>.types.ts
  schemas/
    api/
  components/
    ui/
    blocks/
    layout/
    pages/
      <page>/
        index.tsx
        <page-support>.tsx
  routes/
    __root.ts
    <route>.ts
  styles/
    0-theme.css
    base.css
    shell.css
  tests/
```

`worker-configuration.d.ts`, SQL migrations under `drizzle/`, and `routeTree.gen.ts` produce warnings when their generators have not yet emitted output. `src/lib/api/v1.d.ts` and the Better Auth schema are required generated contracts when their source systems are present.

## Server Layers

`src/server/` is server-only. Browser and shared modules must not import it.

- `api/` owns the mounted Hono OpenAPI surface. Endpoint modules live in `routes/`; API middleware lives in `middleware/`.
- `auth/` owns request-scoped Better Auth construction. `better-auth.ts` exports only `createAuth` and its `Auth` type.
- `config/` owns environment parsing, URLs, feature flags, and runtime configuration.
- `cloudflare/` owns binding and platform adapters.
- `clients/` wraps external SDKs so credentials, retries, logging, and vendor types stay behind server adapters.
- `domain/` owns business workflows. It may call repositories and server clients but does not depend on Hono, React, Cloudflare transport details, or Drizzle schema/query mechanics.
- `db/roles.ts` owns `withUserRole(...)`, `withServerRole(...)`, and the branded user/server/union transaction types. `db/schema/` owns Drizzle declarations. `db/repositories/` owns raw database imports and queries; exported repository functions accept a role-scoped transaction. Repositories are persistence adapters and do not import API, domain, auth, external-client, or request-context layers.

Do not recreate the old `queries/` and `services/` database split. Put persistence in repositories and orchestration in `src/server/domain`.

## API And Shared Layers

Hono route definitions use `createRoute(...)`, declare response schemas, and are mounted through `app.openapi(...)`. Normal app paths are versioned under `/api/v1/...`; the supported exceptions are Better Auth under `/api/auth/*`, `/api/health`, and `/api/v1/openapi.json`.

`pnpm run gen:api` writes the OpenAPI `paths` contract to `src/lib/api/v1.d.ts`. `src/lib/api/client.ts` is the browser-facing generated client boundary.

`src/lib/-hooks` and `src/lib/-schema` hold generic shared code. Named module folders may contain only `<module>.client.ts`, `<module>.hooks.ts`, `<module>.schema.ts`, and `<module>.types.ts` files. Direct TypeScript files under `src/lib` are not allowed.

TanStack server functions are not part of this stack. Do not add `src/lib/-functions`, `*.functions.ts`, `createServerFn`, or TanStack server-function package imports. Expose server workflows through Hono OpenAPI routes and consume them through the generated API client.

SSR route data may use `createIsomorphicFn` as a transport adapter without creating a second application API. The `.server(...)` branch may dynamically import one `src/server/api/*.loader.ts` adapter that invokes the in-process Hono API with the request context; the `.client(...)` branch calls `src/lib/api/client.ts`. Server loaders return the same validated DTO exposed by OpenAPI and never contain product workflows, authorization decisions, or database queries.

## React And Route Boundaries

All `.tsx` files live under `src/components/`. Route files, `src/router.ts`, server code, generated contracts, and shared modules remain `.ts`. A route that needs to render a component from a `.ts` file can use `createElement`.

Page packages live under `components/pages/<page>/`. Their `index.tsx` exports exactly one default React component. Only routes and Storybook stories import a page package entrypoint; only that package index imports its private support files.

`components/blocks` contains stateless, route-agnostic compositions. Blocks receive data and callbacks through props and do not import route state, workflow hooks, pages, layouts, or server code. Generic controls belong in `components/ui`; shells and navigation belong in `components/layout`; page-specific forms and stateful workflows belong in the owning page package. `components/forms` is not allowed.

Routes stay thin and do not contain local `components`, `hooks`, `schemas`, `utils`, or `lib` directories. Only route files read TanStack route state; they pass plain props to page components. Components may use rendering and navigation primitives but do not call route-state APIs such as `useParams`.

## Source Directory Allowlist

Direct child directories are restricted:

- `src/`: `server`, `lib`, `schemas`, `components`, `routes`, `styles`, `tests`
- `src/server/`: `api`, `auth`, `config`, `cloudflare`, `clients`, `domain`, `db`
- `src/server/api/`: `routes`, `middleware`
- `src/server/db/`: `schema`, `repositories`
- `src/components/`: `ui`, `blocks`, `layout`, `pages`
- `src/schemas/`: `api`

Use `ultralint --list-rules` for the current enforcement catalog and `ultralint . --explain-config` before diagnosing a slot or layout finding.
