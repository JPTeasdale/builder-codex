# ultralint

Rust CLI for enforcing the executable Edge Service Stack structure and contract policy.

## Install

Run the installer from the project that should receive the package script:

```bash
/Users/jpteasdale/code/builder-codex/scripts/install-ultralint.sh
```

The installer runs `cargo install --locked --force`, prints `ultralint --version`, and uses `pnpm pkg set` when the current directory contains `package.json`.

## Commands

```bash
ultralint .
ultralint . --json
ultralint . --deny-warnings
ultralint --list-rules
ultralint --list-rules --json
ultralint . --explain-config
ultralint . --explain-config --json
ultralint --version
```

For source-tree development without installing:

```bash
cargo run --manifest-path /Users/jpteasdale/code/builder-codex/tools/ultralint/Cargo.toml -- <project-root>
```

`--version` prints both the tool and policy versions. `--explain-config` prints the detected web, Worker, mobile, and shared-package slots plus the built-in policy inputs.

## Executable Contract

The canonical folder explanation lives in [`knowledge/stacks/edge-service-stack/directory-structure.md`](../../knowledge/stacks/edge-service-stack/directory-structure.md). The high-level enforced boundaries are:

- Hono OpenAPI handlers live under `src/server/api`; normal app endpoints use `/api/v1/...`.
- The mounted API installs one `onError` boundary and never returns raw error objects, messages, stacks, or causes.
- OpenAPI generation writes `src/lib/api/v1.d.ts`, and browser API access goes through `src/lib/api/client.ts`.
- TanStack server functions, `createServerFn`, `src/lib/-functions`, and `*.functions.ts` are forbidden.
- `.tsx` files are allowed only under `src/components`; route, router, shared, and server files remain `.ts`.
- `src/server/db/roles.ts` opens user/server transactions; repository functions accept `UserTransaction`, `ServerTransaction`, or `RoleTransaction` and keep runtime dependencies inside `src/server/db`.
- Worker postgres-js clients use Hyperdrive with `max: 1`; RLS policy helpers are centralized and generated migrations are the only deploy path.
- Permission strings use `<table-resource>:<action>:<scope>` and direct role comparisons stay inside centralized RBAC ownership.
- Preview and production use separate mutable Cloudflare resources; queue, scheduled, and Durable Object bindings have matching runtime handlers and migration contracts.
- Package and generation commands use pnpm.

There is no project-local configuration. App slots and policy are inferred from the project root, `apps/*`, `packages/*`, `package.json`, `vite.config.ts`, `wrangler.jsonc`, route/router files, and `capacitor.config.ts`.

## Findings And Exit Codes

- Errors cause exit code `1`.
- Warnings are reported but exit `0` by default.
- `--deny-warnings` makes warnings cause exit code `1`.
- CLI, filesystem, and analyzer failures use exit code `2`.
- `--json` emits the versioned report schema with tool version, policy version, summary counts, and findings.

## Suppressions

Use a narrow inline directive on the finding line or immediately above it:

```ts
// ultralint: allow route-thinness -- temporary generated adapter until=2026-12-31
```

The directive must name one current rule and include a concrete reason. `until=YYYY-MM-DD` is optional. Blanket, unknown, malformed, and expired suppressions are errors; unused suppressions are warnings. Run `ultralint --list-rules` before adding one.

## Rule Catalog

The executable catalog is the generated, current source of truth:

```bash
ultralint --list-rules
ultralint --list-rules --json
```

Do not maintain a copied rule list in this README.

## Adding Rules

Add a module in `src/rules`, implement `Rule`, register it in `builtin_rules()`, and update focused Rust fixtures in the normal ultralint development workflow.
