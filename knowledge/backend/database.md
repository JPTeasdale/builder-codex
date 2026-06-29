# Database

- Treat schema migrations as approval-gated unless the user explicitly authorizes them.
- Prefer additive migrations for production systems.
- Check indexes and query plans for high-cardinality or frequently accessed paths.
- Never run destructive data changes without explicit approval and a rollback plan.
