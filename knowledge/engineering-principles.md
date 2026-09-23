# Engineering Principles

- Make the smallest coherent change that solves the ticket.
- Read existing code before inventing abstractions.
- Prefer local conventions over generic framework advice.
- Keep behavior changes covered by focused tests when practical.
- Separate deterministic checks from agentic judgment.
- Record assumptions in `.agent/plan.md` when the ticket is ambiguous.
- Stop at approval gates for deploys, migrations, destructive actions, and irreversible external effects.
### Project Setup
- Always use cli tools to generate configs and instal dependencies. 
- Always use cli too