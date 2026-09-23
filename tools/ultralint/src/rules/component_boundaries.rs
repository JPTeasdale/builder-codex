use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_ts_source_file, join};
use crate::rules::{Report, Rule};

pub struct ComponentBoundariesRule;

impl Rule for ComponentBoundariesRule {
    fn id(&self) -> &'static str {
        "component-boundaries"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "keeps component subfolders aligned to their intended UI responsibilities"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) {
                continue;
            }

            if let Some(ts) = &file.ts {
                for import in &ts.imports {
                    let resolved = project
                        .resolve_import(&file.rel_path, &import.source)
                        .unwrap_or_else(|| import.source.clone());
                    let Some(target) = page_target_from_project_path(&resolved, config)
                        .or_else(|| page_target_from_alias(&import.source))
                    else {
                        continue;
                    };

                    if target.kind == PageImportKind::Entrypoint
                        && !is_route_file(&file.rel_path, config)
                        && !is_storybook_file(&file.rel_path)
                    {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(import.line),
                            "non-route code imports a page package entrypoint",
                            "only src/routes and Storybook stories may import src/components/pages/<page>; move shared UI to components/blocks or components/ui.",
                        );
                    }

                    if target.kind == PageImportKind::Internal
                        && !is_page_index_for(&file.rel_path, &target.page, config)
                    {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(import.line),
                            "page internal component is imported outside its page index",
                            "only src/components/pages/<page>/index.tsx may import support files from that page package. Import the page package entrypoint from routes or stories instead.",
                        );
                    }
                }
            }

            if is_forms_component(&file.rel_path, config) {
                report.error(
                    self.id(),
                    &file.rel_path,
                    None,
                    "src/components/forms is not an allowed component boundary",
                    "put page-specific forms and form workflows in src/components/pages; put generic form controls in src/components/ui; keep reusable stateless compositions in src/components/blocks.",
                );
                continue;
            }

            if is_direct_page_file(&file.rel_path, config) {
                report.error(
                    self.id(),
                    &file.rel_path,
                    None,
                    "page component is defined directly under src/components/pages",
                    "create a page package like src/components/pages/<page>/index.tsx; keep page-specific support components beside that entrypoint inside the same package.",
                );
                continue;
            }

            if is_page_index_file(&file.rel_path, config) {
                check_page_index_exports(file, report, self.id());
            }

            if !is_blocks_component(&file.rel_path, config) {
                continue;
            }

            if let Some(ts) = &file.ts {
                for import in &ts.imports {
                    let resolved = project
                        .resolve_import(&file.rel_path, &import.source)
                        .unwrap_or_else(|| import.source.clone());
                    if is_tanstack_router_import(&import.source) {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(import.line),
                            "block component imports TanStack route/navigation primitives",
                            "blocks must be stateless and route-agnostic. Keep route state, links, outlets, and navigation wiring in src/components/pages or src/components/layout.",
                        );
                    }

                    if is_data_workflow_import(&import.source) {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(import.line),
                            "block component imports data/workflow hooks",
                            "blocks should receive data and callbacks through props. Keep data fetching, mutations, and form workflows in src/components/pages.",
                        );
                    }

                    if let Some(layer) = forbidden_block_import_layer(&resolved) {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(import.line),
                            format!("block component imports {layer} code"),
                            "blocks may compose ui primitives and other blocks, but must not import pages, layout shells, server-only code, hooks, or server functions.",
                        );
                    }
                }

                for call in &ts.calls {
                    if !uses_stateful_hook(&call.callee) {
                        continue;
                    }
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(call.line),
                        "block component uses React state/effect workflow hooks",
                        "blocks must be stateless: pass state, derived data, event handlers, and side effects in from src/components/pages.",
                    );
                }
            }
        }

        for root in &config.web_apps {
            check_page_packages(project, root, report, self.id());
        }
    }
}

fn is_forms_component(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/components/forms/"))
}

fn is_blocks_component(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/components/blocks/"))
}

fn is_direct_page_file(rel_path: &str, config: &Config) -> bool {
    if !is_ts_source_file(rel_path) {
        return false;
    }

    config.web_apps.iter().any(|root| {
        let prefix = join(root, "src/components/pages/");
        let Some(rest) = rel_path.strip_prefix(&prefix) else {
            return false;
        };
        !rest.contains('/')
    })
}

fn check_page_packages(project: &Project, root: &str, report: &mut Report, rule_id: &'static str) {
    let prefix = join(root, "src/components/pages/");
    let mut page_dirs = Vec::<String>::new();

    for file in &project.files {
        if !is_ts_source_file(&file.rel_path) {
            continue;
        }
        let Some(rest) = file.rel_path.strip_prefix(&prefix) else {
            continue;
        };
        let Some((page_dir, _)) = rest.split_once('/') else {
            continue;
        };
        if !page_dirs.iter().any(|existing| existing == page_dir) {
            page_dirs.push(page_dir.to_string());
        }
    }

    for page_dir in page_dirs {
        let index_path = format!("{prefix}{page_dir}/index.tsx");
        if project.exists(&index_path) {
            continue;
        }
        report.error(
            rule_id,
            format!("{prefix}{page_dir}"),
            None,
            "page package is missing index.tsx",
            "routes import page package entrypoints only. Add src/components/pages/<page>/index.tsx and keep page-specific support components beside it.",
        );
    }
}

fn check_page_index_exports(
    file: &crate::fs::ProjectFile,
    report: &mut Report,
    rule_id: &'static str,
) {
    let mut default_exports = 0;

    for (index, line) in file.text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }

        if trimmed.starts_with("export default ") {
            default_exports += 1;
            if !is_default_component_export(trimmed) {
                report.error(
                    rule_id,
                    &file.rel_path,
                    Some(line_number),
                    "page index default export is not a component",
                    "page package indexes should export exactly one default React component, for example `export default function ApplyPage(...) { ... }`.",
                );
            }
            continue;
        }

        if trimmed.starts_with("export ") {
            report.error(
                rule_id,
                &file.rel_path,
                Some(line_number),
                "page index has a non-default export",
                "page package indexes may export exactly one default component. Keep types, helpers, and support components private to the page package.",
            );
        }
    }

    if default_exports != 1 {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            format!("page index has {default_exports} default exports"),
            "page package indexes may export exactly one default React component.",
        );
    }
}

fn is_default_component_export(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("export default ") else {
        return false;
    };
    let rest = rest.trim_start();

    if let Some(name) = rest
        .strip_prefix("function ")
        .map(str::trim_start)
        .map(parse_identifier)
    {
        return is_component_name(&name);
    }

    if let Some(name) = rest
        .strip_prefix("async function ")
        .map(str::trim_start)
        .map(parse_identifier)
    {
        return is_component_name(&name);
    }

    is_component_name(&parse_identifier(rest))
}

fn parse_identifier(value: &str) -> String {
    value
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$')
        .collect()
}

fn is_component_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PageImportTarget {
    page: String,
    kind: PageImportKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageImportKind {
    Entrypoint,
    Internal,
}

fn page_target_from_alias(specifier: &str) -> Option<PageImportTarget> {
    for prefix in [
        "~/components/pages/",
        "@/components/pages/",
        "src/components/pages/",
    ] {
        if let Some(rest) = specifier.strip_prefix(prefix) {
            return classify_page_import_rest(rest);
        }
    }

    let marker = "/components/pages/";
    let start = specifier.find(marker)? + marker.len();
    classify_page_import_rest(&specifier[start..])
}

fn page_target_from_project_path(path: &str, config: &Config) -> Option<PageImportTarget> {
    for root in &config.web_apps {
        let prefix = join(root, "src/components/pages/");
        if let Some(rest) = path.strip_prefix(&prefix) {
            return classify_page_import_rest(rest);
        }
    }
    None
}

fn classify_page_import_rest(rest: &str) -> Option<PageImportTarget> {
    let clean = rest
        .trim_matches('/')
        .trim_end_matches(".tsx")
        .trim_end_matches(".ts");
    if clean.is_empty() {
        return None;
    }

    let parts = clean
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let page = parts.first()?.to_string();

    let kind = match parts.as_slice() {
        [_page] => PageImportKind::Entrypoint,
        [_page, "index"] => PageImportKind::Entrypoint,
        [_page, ..] => PageImportKind::Internal,
        [] => return None,
    };

    Some(PageImportTarget { page, kind })
}

fn is_route_file(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/routes/"))
}

fn is_storybook_file(rel_path: &str) -> bool {
    rel_path.ends_with(".stories.ts")
        || rel_path.ends_with(".stories.tsx")
        || rel_path.ends_with(".story.ts")
        || rel_path.ends_with(".story.tsx")
}

fn is_page_index_file(rel_path: &str, config: &Config) -> bool {
    config.web_apps.iter().any(|root| {
        let prefix = join(root, "src/components/pages/");
        let Some(rest) = rel_path.strip_prefix(&prefix) else {
            return false;
        };
        rest.split('/').count() == 2 && rest.ends_with("/index.tsx")
    })
}

fn is_page_index_for(rel_path: &str, page: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| rel_path == join(root, &format!("src/components/pages/{page}/index.tsx")))
}

fn is_tanstack_router_import(specifier: &str) -> bool {
    matches!(
        specifier,
        "@tanstack/react-router" | "@tanstack/start" | "@tanstack/react-start"
    )
}

fn is_data_workflow_import(specifier: &str) -> bool {
    matches!(
        specifier,
        "@tanstack/react-query"
            | "@tanstack/react-form"
            | "react-hook-form"
            | "formik"
            | "final-form"
            | "react-final-form"
    )
}

fn forbidden_block_import_layer(specifier: &str) -> Option<&'static str> {
    if matches_layer_import(specifier, "components/layout") {
        return Some("layout");
    }
    if matches_layer_import(specifier, "components/pages") {
        return Some("page");
    }
    if matches_layer_import(specifier, "server") {
        return Some("server-only");
    }
    if matches_layer_import(specifier, "lib/-hooks") || specifier.contains(".hooks") {
        return Some("hook");
    }
    None
}

fn matches_layer_import(specifier: &str, layer: &str) -> bool {
    specifier == format!("~/{layer}")
        || specifier.starts_with(&format!("~/{layer}/"))
        || specifier == format!("@/{layer}")
        || specifier.starts_with(&format!("@/{layer}/"))
        || specifier == format!("src/{layer}")
        || specifier.starts_with(&format!("src/{layer}/"))
        || specifier.ends_with(&format!("/{layer}"))
        || specifier.contains(&format!("/{layer}/"))
}

fn uses_stateful_hook(callee: &str) -> bool {
    STATEFUL_HOOKS.contains(&callee)
}

const STATEFUL_HOOKS: &[&str] = &[
    "useState",
    "useReducer",
    "useEffect",
    "useLayoutEffect",
    "useInsertionEffect",
    "useSyncExternalStore",
    "useTransition",
    "useOptimistic",
    "useActionState",
];
