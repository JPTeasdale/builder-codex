use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_test_file, is_ts_source_file};
use crate::rules::{Report, Rule};

pub struct ServerLayerBoundariesRule;

impl Rule for ServerLayerBoundariesRule {
    fn id(&self) -> &'static str {
        "server-layer-boundaries"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "enforces domain, repository, API, and external client dependency direction"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) || is_test_file(&file.rel_path) {
                continue;
            }

            let layer = server_layer(&file.rel_path, config);
            if let Some(ts) = &file.ts {
                for import in &ts.imports {
                    let target = project
                        .resolve_import(&file.rel_path, &import.source)
                        .unwrap_or_else(|| import.source.clone());
                    check_import(
                        layer,
                        &file.rel_path,
                        import.line,
                        &import.source,
                        &target,
                        report,
                        self.id(),
                    );
                }
            }
            for (index, line) in file.text.lines().enumerate() {
                let line_number = index + 1;
                if layer != ServerLayer::Repository
                    && layer != ServerLayer::Schema
                    && layer != ServerLayer::Domain
                    && contains_drizzle_row_type(line)
                {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "Drizzle inferred row type leaks outside the server data layer",
                        "Do not export or import `$inferSelect`, `$inferInsert`, `InferSelectModel`, or `InferInsertModel` from browser/shared code. Convert database rows to explicit domain/API DTO types in src/server/domain or repository return types, then expose those contracts through OpenAPI and src/lib/api/client.ts.",
                    );
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerLayer {
    Api,
    Domain,
    Repository,
    Schema,
    ClientAdapter,
    Other,
}

fn server_layer(rel_path: &str, config: &Config) -> ServerLayer {
    for root in &config.web_apps {
        if has_prefix(rel_path, root, "src/server/api/") {
            return ServerLayer::Api;
        }
        if has_prefix(rel_path, root, "src/server/domain/") {
            return ServerLayer::Domain;
        }
        if has_prefix(rel_path, root, "src/server/db/repositories/") {
            return ServerLayer::Repository;
        }
        if has_prefix(rel_path, root, "src/server/db/schema/") {
            return ServerLayer::Schema;
        }
        if has_prefix(rel_path, root, "src/server/clients/") {
            return ServerLayer::ClientAdapter;
        }
    }
    ServerLayer::Other
}

fn check_import(
    layer: ServerLayer,
    rel_path: &str,
    line: usize,
    specifier: &str,
    target: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    if layer == ServerLayer::Domain
        && (is_api_import(target)
            || is_transport_import(specifier)
            || is_react_import(specifier)
            || is_db_schema_import(target)
            || is_cloudflare_binding_import(specifier, target))
    {
        report.error(
            rule_id,
            rel_path,
            Some(line),
            "domain layer imports transport or persistence details",
            "src/server/domain is business workflow code. It may import repositories, server clients, auth abstractions, config, and shared contracts, but must not import Hono/API modules, React, Cloudflare binding/platform types, Request/Response transport, or db/schema. Raw Drizzle/query mechanics are enforced by db-access-layer.",
        );
    }

    if layer == ServerLayer::Repository
        && (is_domain_import(target)
            || is_api_import(target)
            || target.contains("/server/clients/")
            || target.starts_with("src/server/clients/")
            || target.contains("/src/components/")
            || target.starts_with("src/components/"))
    {
        report.error(
            rule_id,
            rel_path,
            Some(line),
            "repository imports code from a higher layer",
            "Repositories should be dumb persistence adapters. They may import src/server/db/schema, Drizzle, and pure shared types only; move orchestration/business logic to src/server/domain.",
        );
    }

    if is_external_sdk(specifier) && layer != ServerLayer::ClientAdapter {
        report.error(
            rule_id,
            rel_path,
            Some(line),
            "external SDK is imported outside src/server/clients",
            "Wrap external SDKs in src/server/clients/<service>.ts so credentials, retries, logging, and vendor-specific types stay behind one adapter. Install SDKs with pnpm, e.g. `pnpm add twilio`, `pnpm add stripe`, or `pnpm add resend`.",
        );
    }
}

fn is_api_import(specifier: &str) -> bool {
    specifier.starts_with("src/server/api") || specifier.contains("/src/server/api/")
}

fn is_domain_import(specifier: &str) -> bool {
    specifier.starts_with("src/server/domain") || specifier.contains("/src/server/domain/")
}

fn is_db_schema_import(specifier: &str) -> bool {
    specifier.starts_with("src/server/db/schema") || specifier.contains("/src/server/db/schema/")
}

fn is_transport_import(specifier: &str) -> bool {
    specifier == "hono" || specifier.starts_with("hono/") || specifier == "@hono/zod-openapi"
}

fn is_react_import(specifier: &str) -> bool {
    specifier == "react" || specifier.starts_with("@tanstack/react")
}

fn is_cloudflare_binding_import(specifier: &str, target: &str) -> bool {
    target.starts_with("src/server/cloudflare")
        || target.contains("/src/server/cloudflare/")
        || specifier == "cloudflare:workers"
        || specifier.starts_with("@cloudflare/")
}

fn is_external_sdk(specifier: &str) -> bool {
    matches!(
        specifier,
        "twilio" | "stripe" | "resend" | "@sendgrid/mail" | "openai"
    )
}

fn contains_drizzle_row_type(line: &str) -> bool {
    line.contains("$inferSelect")
        || line.contains("$inferInsert")
        || line.contains("InferSelectModel")
        || line.contains("InferInsertModel")
}
