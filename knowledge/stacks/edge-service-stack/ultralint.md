# Ultralint

Sources:

- `/Users/jpteasdale/code/builder-codex/tools/ultralint`
- `/Users/jpteasdale/code/builder-codex/tools/ultralint/README.md`

`ultralint` is the Rust CLI for enforcing Edge Service Stack structure and contract policy. It catches executable architecture drift; it does not replace review of product intent, UX quality, security posture, or current third-party guidance.

The canonical folder contract is [[stacks/edge-service-stack/directory-structure]]. Do not duplicate its tree in topic notes.

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

Use `pnpm run ultralint` when the project package script is present. `--version` reports tool and policy versions. `--explain-config` reports detected app slots and built-in policy inputs.

Install or update without changing project-local policy:

```bash
/Users/jpteasdale/code/builder-codex/scripts/install-ultralint.sh
pnpm pkg set scripts.ultralint="ultralint ."
```

## Rule Catalog

`ultralint --list-rules` is the generated, current rule catalog. Use `--json` when another tool needs rule IDs, categories, and descriptions. Do not maintain a copied list in knowledge pages.

## Findings

- Errors exit `1`.
- Warnings are visible but exit `0` by default.
- `--deny-warnings` makes warnings exit `1`.
- CLI, filesystem, and analyzer failures exit `2`.
- `--json` emits the versioned report contract with tool version, policy version, counts, severity, rule ID, category, path, line, message, and help.

Warnings identify recommended or stale generated state without blocking the default run. Use `--deny-warnings` in gates that require a completely clean report.

## Suppressions

Use an inline directive on the finding line or immediately above it:

```ts
// ultralint: allow route-thinness -- temporary generated adapter until=2026-12-31
```

Name one exact current rule and provide a concrete reason. `until=YYYY-MM-DD` is optional. Blanket, unknown, reasonless, malformed, and expired suppressions are errors. Unused suppressions are warnings and should be removed. Suppressions do not create project-local policy.

## Policy

There is no `.ultralint.toml` or other project-local configuration. The executable infers root and workspace slots and applies one built-in policy. If a policy is wrong, update the tool and its focused fixtures rather than weakening one project.
