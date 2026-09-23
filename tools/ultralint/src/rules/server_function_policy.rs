use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_test_file, is_ts_source_file};
use crate::rules::{Report, Rule};

pub struct ServerFunctionPolicyRule;

impl Rule for ServerFunctionPolicyRule {
    fn id(&self) -> &'static str {
        "server-function-policy"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "forbids TanStack server functions in API-only projects"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_app_source(&file.rel_path, config) {
                continue;
            }

            if is_legacy_functions_path(&file.rel_path) {
                report.error(
                    self.id(),
                    &file.rel_path,
                    None,
                    "src/lib/-functions is not allowed",
                    "This stack uses Hono OpenAPI routes plus the generated API client, not TanStack server functions. Move server workflows to src/server/domain, persistence to src/server/db/repositories, and expose HTTP through src/server/api/routes/*.route.ts.",
                );
            }

            if !is_ts_source_file(&file.rel_path) || is_test_file(&file.rel_path) {
                continue;
            }

            if file.rel_path.ends_with(".functions.ts") || file.rel_path.ends_with(".functions.tsx")
            {
                report.error(
                    self.id(),
                    &file.rel_path,
                    None,
                    "TanStack server function file suffix is not allowed",
                    "This stack uses Hono OpenAPI routes plus the generated API client, not TanStack server functions. Move server workflows to src/server/domain, persistence to src/server/db/repositories, and expose HTTP through src/server/api/routes/*.route.ts. Remove `.functions.ts` files instead of adding new ones.",
                );
            }

            for (index, line) in file.text.lines().enumerate() {
                let line_number = index + 1;
                if line.contains("@tanstack/react-start/server-functions")
                    || line.contains("@tanstack/start/server")
                    || line.contains("@tanstack/server-functions")
                {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "TanStack server functions package is not allowed",
                        "Use Hono OpenAPI routes under src/server/api/routes and call them from src/lib/api/client.ts. Install route dependencies with `pnpm add hono @hono/zod-openapi zod` and client dependencies with `pnpm add openapi-fetch openapi-react-query`.",
                    );
                }

                if contains_legacy_function_import(line) {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "legacy server function import is not allowed",
                        "Do not import from src/lib/-functions or *.functions.ts. Move the server workflow to src/server/domain, expose it through a Hono OpenAPI route, then call it with src/lib/api/client.ts.",
                    );
                }

                if contains_identifier_call(line, "createServerFn")
                    || imports_identifier(line, "createServerFn")
                {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "createServerFn is not allowed",
                        "Replace TanStack server functions with Hono OpenAPI endpoints. Define a createRoute(...) schema in src/server/api/routes/*.route.ts, mount it with app.openapi(...), regenerate src/lib/api/v1.d.ts with `pnpm gen:api`, then call useApiQuery/useApiMutation from src/lib/api/client.ts.",
                    );
                }
            }
        }
    }
}

fn is_legacy_functions_path(rel_path: &str) -> bool {
    rel_path.contains("/src/lib/-functions/") || rel_path.starts_with("src/lib/-functions/")
}

fn contains_legacy_function_import(line: &str) -> bool {
    line.contains("lib/-functions") || line.contains(".functions")
}

fn is_app_source(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/"))
}

fn contains_identifier_call(line: &str, name: &str) -> bool {
    line.contains(&format!("{name}("))
}

fn imports_identifier(line: &str, name: &str) -> bool {
    let compact = line.replace(' ', "");
    compact.starts_with("import{") && compact.contains(name)
}
