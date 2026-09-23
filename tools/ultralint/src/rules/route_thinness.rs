use crate::analysis::{ImportFact, ImportKind};
use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{DeclarationKind, is_route_file, parse_top_level_declaration};
use crate::rules::{Report, Rule};

pub struct RouteThinnessRule;

const MAX_SUBSTANTIVE_ROUTE_LINES: usize = 180;

impl Rule for RouteThinnessRule {
    fn id(&self) -> &'static str {
        "route-thinness"
    }

    fn category(&self) -> &'static str {
        "react"
    }

    fn description(&self) -> &'static str {
        "keeps route files focused on routing, data hooks, and page composition"
    }
    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_route_file(&file.rel_path, config) {
                continue;
            }

            let mut substantive_lines = 0;
            let mut page_imports = 0;
            let dynamic_route = is_dynamic_route(&file.rel_path, &file.text);
            let page_component_names = file
                .ts
                .as_ref()
                .into_iter()
                .flat_map(|ts| &ts.imports)
                .filter(|import| is_page_entrypoint_import(&import.source))
                .flat_map(|import| import.local_names.iter().cloned())
                .collect::<Vec<_>>();
            if let Some(ts) = &file.ts {
                for import in &ts.imports {
                    let specifier = import.source.as_str();
                    if is_page_entrypoint_import(specifier) {
                        page_imports += 1;
                    } else if is_page_component_import(specifier) {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(import.line),
                            "route file imports page internals",
                            "routes may import one page package entrypoint from src/components/pages/<page> or src/components/pages/<page>/index; keep page support components inside that page package.",
                        );
                    } else if is_local_source_import(specifier)
                        && !is_allowed_route_data_import(import)
                    {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(import.line),
                            "route file imports local code outside src/components/pages",
                            "routes may import a single page package entrypoint from src/components/pages/<page>; move composition into that page package",
                        );
                    }
                }
            }
            for (index, line) in file.text.lines().enumerate() {
                let line_number = index + 1;
                if is_substantive_line(line) {
                    substantive_lines += 1;
                }

                if dynamic_route && uses_direct_page_component(line, &page_component_names) {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "dynamic route mounts a page component directly",
                        "dynamic routes should read params/search in a tiny route adapter and pass plain props to the page component, e.g. `component: function RouteComponent() { const params = Route.useParams(); return createElement(Page, params); }`",
                    );
                }

                if declares_schema(line) {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "schema or validator appears to be declared in a route file",
                        "move shared schemas to src/lib/-schema/* or src/lib/<module>/<module>.schema.ts; generated API contracts belong in src/lib/api/v1.d.ts and should be produced by `pnpm gen:api`.",
                    );
                }

                let Some(decl) = parse_top_level_declaration(line, &file.rel_path, line_number)
                else {
                    continue;
                };

                if is_component_declaration(&decl.kind, &decl.name)
                    && !is_allowed_route_component_name(&decl.name)
                {
                    report.error(
                        self.id(),
                        &decl.file,
                        Some(decl.line),
                        format!("component `{}` is declared directly in a route file", decl.name),
                        "move page UI to src/components/pages and keep the route as routing plus a single page component import",
                    );
                }
            }

            if substantive_lines > MAX_SUBSTANTIVE_ROUTE_LINES {
                report.warning(
                    self.id(),
                    &file.rel_path,
                    None,
                    format!(
                        "route file has {substantive_lines} substantive lines; expected {MAX_SUBSTANTIVE_ROUTE_LINES} or fewer"
                    ),
                    "move UI to components/, shared module code to lib/, schemas to lib/-schema or module .schema.ts files, and server-only work to server/ behind Hono API handlers",
                );
            }

            if page_imports > 1 {
                report.error(
                    self.id(),
                    &file.rel_path,
                    None,
                    "route file imports more than one page component",
                    "routes should import a single page package entrypoint from src/components/pages/<page> and leave composition inside that page package",
                );
            }
        }
    }
}

fn is_component_declaration(kind: &DeclarationKind, name: &str) -> bool {
    matches!(
        kind,
        DeclarationKind::Function | DeclarationKind::Const | DeclarationKind::Class
    ) && name
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn is_allowed_route_component_name(name: &str) -> bool {
    matches!(
        name,
        "Route"
            | "RouteComponent"
            | "Component"
            | "PendingComponent"
            | "ErrorComponent"
            | "NotFoundComponent"
    )
}

fn declares_schema(line: &str) -> bool {
    line.contains("z.object(")
        || line.contains("z.array(")
        || line.contains("z.discriminatedUnion(")
        || line.contains("createInsertSchema(")
        || line.contains("createSelectSchema(")
        || line.contains("createUpdateSchema(")
}

fn is_dynamic_route(rel_path: &str, text: &str) -> bool {
    rel_path.contains('$') || text.contains("/$")
}

fn uses_direct_page_component(line: &str, page_component_names: &[String]) -> bool {
    let Some((_, rhs)) = line.split_once("component:") else {
        return false;
    };
    let rhs = rhs.trim_start();
    page_component_names
        .iter()
        .any(|name| starts_with_identifier(rhs, name))
}

fn starts_with_identifier(text: &str, identifier: &str) -> bool {
    let Some(rest) = text.strip_prefix(identifier) else {
        return false;
    };
    rest.chars()
        .next()
        .is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'))
}

fn is_page_component_import(specifier: &str) -> bool {
    specifier.starts_with("~/components/pages/")
        || specifier.starts_with("@/components/pages/")
        || specifier.starts_with("src/components/pages/")
        || specifier.contains("/components/pages/")
}

fn is_page_entrypoint_import(specifier: &str) -> bool {
    let Some(rest) = page_import_rest(specifier) else {
        return false;
    };
    let rest = rest.trim_matches('/');
    if rest.is_empty() {
        return false;
    }

    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    let Some(page_dir) = parts.next() else {
        return false;
    };
    if page_dir == "index" || page_dir.ends_with(".tsx") || page_dir.ends_with(".ts") {
        return false;
    }

    matches!(
        (parts.next(), parts.next()),
        (None, None) | (Some("index"), None)
    )
}

fn page_import_rest(specifier: &str) -> Option<&str> {
    for prefix in [
        "~/components/pages/",
        "@/components/pages/",
        "src/components/pages/",
    ] {
        if let Some(rest) = specifier.strip_prefix(prefix) {
            return Some(rest);
        }
    }

    let marker = "/components/pages/";
    let start = specifier.find(marker)? + marker.len();
    Some(&specifier[start..])
}

fn is_local_source_import(specifier: &str) -> bool {
    specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with("~/")
        || specifier.starts_with("@/")
        || specifier.starts_with("src/")
}

fn is_allowed_route_data_import(import: &ImportFact) -> bool {
    is_generated_api_client_import(&import.source)
        || (import.kind == ImportKind::Dynamic && is_server_loader_import(&import.source))
}

fn is_generated_api_client_import(specifier: &str) -> bool {
    matches!(
        specifier,
        "~/lib/api/client" | "@/lib/api/client" | "src/lib/api/client"
    ) || specifier.ends_with("/src/lib/api/client")
}

fn is_server_loader_import(specifier: &str) -> bool {
    let normalized = specifier
        .strip_prefix("~/")
        .or_else(|| specifier.strip_prefix("@/"))
        .unwrap_or(specifier);
    (normalized.starts_with("server/api/")
        || normalized.starts_with("src/server/api/")
        || normalized.contains("/src/server/api/"))
        && normalized.ends_with(".loader")
}

fn is_substantive_line(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("//")
        && !trimmed.starts_with("/*")
        && !trimmed.starts_with('*')
}
