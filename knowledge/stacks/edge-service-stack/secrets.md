# Edge Service Stack Secrets

Source: `/Users/jpteasdale/plugins/edge-service-stack/references/secrets-contract.md`

Secrets are classified by where they are allowed to exist.

## Runtime Secrets

Runtime secrets are used by deployed server/Worker code through typed `Env` bindings.

Allowed:

- Worker/server/auth code via `env.SECRET_NAME`.
- Wrangler secrets.
- `secrets.required` documentation in `wrangler.jsonc`.

Not allowed:

- Browser route/component code.
- Mobile app source.
- `import.meta.env`.
- `VITE_` or `PUBLIC_` variables.
- Plain `vars` in `wrangler.jsonc`.

Examples: `BETTER_AUTH_SECRET`, `OPENAI_API_KEY`, `POSTHOG_API_KEY`, `STRIPE_SECRET_KEY`, `WEBHOOK_SIGNING_SECRET`.

## Build Secrets

Build secrets are used by CI or build tooling and should not be present in runtime/server/client source.

Allowed:

- CI secret store.
- Release scripts.
- Build tooling outside app `src`.

Not allowed:

- Any app `src` folder.
- Worker runtime code.
- Browser/mobile source.
- `wrangler.jsonc` vars.
- Public env variables.

Examples: `CLOUDFLARE_API_TOKEN`, `SENTRY_AUTH_TOKEN`, `TURBO_TOKEN`, `VERCEL_TOKEN`.

## Public Environment Values

Public values are bundled into browser/mobile code and must be harmless if viewed by users.

Allowed prefixes:

- `VITE_`
- `PUBLIC_`

Names containing `SECRET`, `TOKEN`, `PRIVATE`, `PASSWORD`, or `KEY` are suspicious even with a public prefix.

`ultralint` does not read project-local config. Secret classes are hard-coded in the Rust policy.
