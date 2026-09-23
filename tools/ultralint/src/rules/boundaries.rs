use crate::analysis::{ImportFact, ImportKind};
use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_test_file, is_ts_source_file, join};
use crate::rules::{Report, Rule};

pub struct ArchitectureBoundariesRule;

impl Rule for ArchitectureBoundariesRule {
    fn id(&self) -> &'static str {
        "architecture-boundaries"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "prevents server-only imports from leaking into shared, UI, route, schema, and client code"
    }
    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) {
                continue;
            }

            let layer = source_layer(&file.rel_path, config);
            let analytics_wrapper = is_analytics_wrapper(&file.rel_path, config);
            let Some(ts) = &file.ts else {
                continue;
            };
            for import in &ts.imports {
                let target = project
                    .resolve_import(&file.rel_path, &import.source)
                    .unwrap_or_else(|| import.source.clone());
                if is_server_import(&target)
                    && !layer.can_import_server()
                    && !is_isomorphic_server_loader_import(layer, import, &target, &file.text)
                {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(import.line),
                        format!("{} imports server-only code", layer.name_for_message()),
                        server_import_help(),
                    );
                }
                if layer.is_schema_like() && is_react_import(&import.source) {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(import.line),
                        "schema/type file imports React",
                        "schema and type files must stay runtime-neutral; move React code to components/, lib/*/*.hooks.ts, or lib/*/*.client.ts.",
                    );
                }
                if layer == SourceLayer::Server && is_component_import(&target) {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(import.line),
                        "server-only code imports React components",
                        "server/ may import server/, lib/, and schemas/ only. Move UI composition to routes/ or components/.",
                    );
                }
                if !analytics_wrapper && import.source == "posthog-js" {
                    report.warning(
						self.id(),
						&file.rel_path,
						Some(import.line),
						"PostHog is imported outside the analytics wrapper",
                        "use typed analytics helper functions so event names and privacy rules stay centralized",
					);
                }
            }
            if layer == SourceLayer::Component {
                for call in &ts.calls {
                    if !calls_route_state_api(&call.callee) {
                        continue;
                    }
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(call.line),
                        "component reads TanStack route state",
                        "only src/routes may call route state APIs like useParams or Route.useParams; pass route data into components as props",
                    );
                }
            }
        }
    }
}

fn is_analytics_wrapper(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/lib/analytics"))
        || config
            .shared_packages
            .iter()
            .any(|root| has_prefix(rel_path, root, "src/analytics"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLayer {
    Server,
    ServerEntrypoint,
    LibClient,
    LibHook,
    LibSchema,
    LibType,
    LibOther,
    Component,
    Route,
    Schema,
    TestOrScript,
    Other,
}

impl SourceLayer {
    fn can_import_server(self) -> bool {
        matches!(
            self,
            Self::Server | Self::ServerEntrypoint | Self::TestOrScript
        )
    }

    fn is_schema_like(self) -> bool {
        matches!(self, Self::LibSchema | Self::LibType | Self::Schema)
    }

    fn name_for_message(self) -> &'static str {
        match self {
            Self::Server => "server file",
            Self::ServerEntrypoint => "server entrypoint",
            Self::LibClient => "client helper",
            Self::LibHook => "React hook",
            Self::LibSchema => "module schema",
            Self::LibType => "module type file",
            Self::LibOther => "shared lib module",
            Self::Component => "React component",
            Self::Route => "route file",
            Self::Schema => "generated/shared schema",
            Self::TestOrScript => "test or script",
            Self::Other => "source file",
        }
    }
}

fn source_layer(rel_path: &str, config: &Config) -> SourceLayer {
    if is_test_file(rel_path)
        || is_script_file(rel_path)
        || rel_path.ends_with("better-auth.config.ts")
    {
        return SourceLayer::TestOrScript;
    }

    for root in &config.web_apps {
        if rel_path == join(root, "src/server.ts") {
            return SourceLayer::ServerEntrypoint;
        }
        if has_prefix(rel_path, root, "src/server/") {
            return SourceLayer::Server;
        }
        if has_prefix(rel_path, root, "src/components/") {
            return SourceLayer::Component;
        }
        if has_prefix(rel_path, root, "src/routes/") {
            return SourceLayer::Route;
        }
        if has_prefix(rel_path, root, "src/schemas/") {
            return SourceLayer::Schema;
        }
        if has_prefix(rel_path, root, "src/lib/") {
            return lib_layer(rel_path);
        }
    }

    if config
        .mobile_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/"))
    {
        return SourceLayer::LibClient;
    }

    SourceLayer::Other
}

fn is_script_file(rel_path: &str) -> bool {
    rel_path.starts_with("scripts/") || rel_path.contains("/scripts/")
}

fn lib_layer(rel_path: &str) -> SourceLayer {
    if rel_path.contains("/-hooks/")
        || rel_path.ends_with(".hooks.ts")
        || rel_path.ends_with(".hooks.tsx")
    {
        SourceLayer::LibHook
    } else if rel_path.contains("/-schema/")
        || rel_path.ends_with(".schema.ts")
        || rel_path.ends_with(".schema.tsx")
    {
        SourceLayer::LibSchema
    } else if rel_path.ends_with(".client.ts") || rel_path.ends_with(".client.tsx") {
        SourceLayer::LibClient
    } else if rel_path.ends_with(".types.ts") || rel_path.ends_with(".types.tsx") {
        SourceLayer::LibType
    } else {
        SourceLayer::LibOther
    }
}

fn is_server_import(specifier: &str) -> bool {
    specifier == "src/server"
        || specifier.starts_with("src/server/")
        || specifier.contains("/src/server/")
}

fn is_isomorphic_server_loader_import(
    layer: SourceLayer,
    import: &ImportFact,
    target: &str,
    source: &str,
) -> bool {
    layer == SourceLayer::Route
        && import.kind == ImportKind::Dynamic
        && is_server_api_loader(target)
        && import_is_inside_isomorphic_server_branch(source, import.line)
}

fn is_server_api_loader(target: &str) -> bool {
    (target.starts_with("src/server/api/") || target.contains("/src/server/api/"))
        && target.ends_with(".loader.ts")
}

fn import_is_inside_isomorphic_server_branch(source: &str, line: usize) -> bool {
    if !source.contains("createIsomorphicFn") {
        return false;
    }

    let prefix = source.lines().take(line).collect::<Vec<_>>().join("\n");
    let Some(server_start) = prefix.rfind(".server(") else {
        return false;
    };
    prefix
        .rfind(".client(")
        .is_none_or(|client_start| client_start < server_start)
}

fn is_component_import(specifier: &str) -> bool {
    specifier == "src/components"
        || specifier.starts_with("src/components/")
        || specifier.contains("/src/components/")
}

fn is_react_import(specifier: &str) -> bool {
    specifier == "react"
        || specifier.starts_with("react/")
        || specifier == "react-dom"
        || specifier.starts_with("react-dom/")
        || specifier == "lucide-react"
}

fn calls_route_state_api(callee: &str) -> bool {
    ROUTE_STATE_HOOKS.contains(&callee)
        || ROUTE_STATE_METHODS
            .iter()
            .any(|method| callee.ends_with(&format!(".{method}")))
}

const ROUTE_STATE_HOOKS: &[&str] = &[
    "useParams",
    "useSearch",
    "useLoaderData",
    "useRouteContext",
    "useMatches",
    "useMatch",
];

const ROUTE_STATE_METHODS: &[&str] =
    &["useParams", "useSearch", "useLoaderData", "useRouteContext"];

fn server_import_help() -> &'static str {
    "Only src/server/**, src/server.ts, tests, scripts, and the Better Auth CLI config may import ~/server/*. The narrow SSR exception is a dynamic src/server/api/*.loader.ts import inside createIsomorphicFn().server(...) in a route; its .client(...) branch must use src/lib/api/client.ts. Put product workflows behind Hono OpenAPI routes and keep ordinary client helpers, hooks, schemas, components, and routes on shared/generated contracts."
}
