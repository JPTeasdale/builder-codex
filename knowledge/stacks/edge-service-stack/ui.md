# Edge Service Stack UI

Source: `/Users/jpteasdale/code/builder-codex/tools/ultralint`

Use TanStack Start, TanStack Router, React Query, Tailwind v4, and shadcn-style primitives. Build the actual application surface unless the product specifically needs a landing page. See [[stacks/edge-service-stack/directory-structure]] for canonical route, component, hook, module, and style paths.

## Component And Route Rules

- Keep every `.tsx` implementation under `src/components`. Route files, `src/router.ts`, hooks, shared modules, and server code stay `.ts`.
- Keep routes thin. A route may read route state and pass plain props to one page-package entrypoint; use `createElement` when rendering from `.ts`.
- Keep page-specific state, forms, data workflows, and support components inside the owning page package.
- Keep blocks stateless and route-agnostic. Pass data and callbacks through props.
- Put generic controls in UI primitives and shells/navigation in layout components. Do not create a generic `components/forms` layer.
- Only route files read params, search, loader data, or route context. Components may use rendering/navigation primitives but do not call route-state hooks.

## Data And Auth

- Use the generated OpenAPI React Query client for app API data and import contracts from `src/lib/api/v1.d.ts` through client helpers.
- Keep mutations close to page workflows and invalidate specific query keys.
- Use route loaders or `beforeLoad` when they improve navigation, but keep server authorization in Hono middleware and RLS. For first-render SSR data, pair a dynamic `src/server/api/*.loader.ts` import inside `createIsomorphicFn().server(...)` with the generated API client in `.client(...)`; both paths must return the same API DTO.
- Do not use TanStack server functions for session lookup or product operations. Expose server work through Hono OpenAPI routes.
- Cover loading, empty, error, success, and permission-denied states.

## Interaction Quality

- Use shadcn/Radix-style primitives for accessible controls and lucide icons inside icon buttons.
- Keep SaaS and operations UI dense, clear, and efficient.
- Ensure text fits across mobile and desktop and does not overlap adjacent controls.
- Preserve stable dimensions for boards, toolbars, counters, and other fixed-format UI.

## E2E Coverage

Add Playwright coverage for the dashboard or landing smoke path, auth, the primary create/edit workflow, protected navigation, and at least one API-driven UI state. Run `pnpm run test:e2e` and inspect browser screenshots when layout, routing, forms, or responsive behavior changes.
