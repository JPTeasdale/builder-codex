# API Patterns

- For Edge Service Stack APIs, start with [[stacks/edge-service-stack/api]].
- Preserve existing request validation and error response conventions.
- Avoid widening authorization or tenant boundaries.
- Keep API changes backward compatible unless the ticket explicitly asks otherwise.
- Add logging where it helps operate failures, but avoid sensitive payloads.
