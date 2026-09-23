use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{DeclarationKind, is_ts_source_file, join, parse_top_level_declaration};
use crate::rules::{Report, Rule};

pub struct BetterAuthImportsRule;

impl Rule for BetterAuthImportsRule {
    fn id(&self) -> &'static str {
        "better-auth"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "enforces the Better Auth Worker factory and CLI config shape"
    }
    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) {
                continue;
            }

            if is_better_auth_config(&file.rel_path, config) {
                check_better_auth_config(file, report, self.id());
                continue;
            }

            if is_allowed_auth_factory(&file.rel_path, config) {
                check_auth_factory_exports(file, report, self.id());
                continue;
            }

            if let Some(analysis) = &file.ts {
                for import in &analysis.imports {
                    if !imports_better_auth_package(&import.source) {
                        continue;
                    }
                    if is_allowed_session_cookie_adapter(&file.rel_path, &import.source, config) {
                        continue;
                    }
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(import.line),
                        "Better Auth package import outside the server auth factory",
                        better_auth_import_help(),
                    );
                }
            }
        }
    }
}

fn check_better_auth_config(
    file: &crate::fs::ProjectFile,
    report: &mut Report,
    rule_id: &'static str,
) {
    let text = file.text.as_str();
    let compact = compact_code(text);

    if !text.contains("createAuth") || !text.contains("src/server/auth/better-auth") {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "better-auth.config.ts must import the auth factory",
            better_auth_config_help(),
        );
    }

    if !compact.contains("exportconstauth=createAuth(") {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "better-auth.config.ts must export `auth` by calling `createAuth`",
            better_auth_config_help(),
        );
    }

    if !text.contains("as unknown as Env") {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "better-auth.config.ts must pass a placeholder Worker Env",
            better_auth_config_help(),
        );
    }

    if !text.contains("as unknown as ExecutionContext") {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "better-auth.config.ts must pass a placeholder ExecutionContext",
            better_auth_config_help(),
        );
    }

    for (index, line) in file.text.lines().enumerate() {
        for export in parse_export_list(line) {
            report.error(
                rule_id,
                &file.rel_path,
                Some(index + 1),
                format!(
                    "better-auth.config.ts re-exports unsupported `{}`",
                    export.name
                ),
                better_auth_config_help(),
            );
        }
    }
}

fn check_auth_factory_exports(
    file: &crate::fs::ProjectFile,
    report: &mut Report,
    rule_id: &'static str,
) {
    let mut has_create_auth = false;
    let mut has_auth_type = false;

    for (index, line) in file.text.lines().enumerate() {
        let line_number = index + 1;

        if let Some(decl) = parse_top_level_declaration(line, &file.rel_path, line_number) {
            if !decl.exported {
                continue;
            }

            if decl.kind == DeclarationKind::Function && decl.name == "createAuth" {
                has_create_auth = true;
                continue;
            }
            if decl.kind == DeclarationKind::Type && decl.name == "Auth" {
                has_auth_type = true;
                continue;
            }

            report.error(
                rule_id,
                &file.rel_path,
                Some(line_number),
                format!("Better Auth factory exports unsupported `{}`", decl.name),
                better_auth_factory_help(),
            );
        }

        for export in parse_export_list(line) {
            if export.is_type && export.name == "Auth" {
                has_auth_type = true;
                continue;
            }
            if !export.is_type && export.name == "createAuth" {
                has_create_auth = true;
                continue;
            }

            report.error(
                rule_id,
                &file.rel_path,
                Some(line_number),
                format!("Better Auth factory exports unsupported `{}`", export.name),
                better_auth_factory_help(),
            );
        }
    }

    if !has_create_auth {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "Better Auth factory must export `createAuth`",
            better_auth_factory_help(),
        );
    }

    if !has_auth_type {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "Better Auth factory must export `Auth`",
            better_auth_factory_help(),
        );
    }
}

fn better_auth_import_help() -> &'static str {
    "Do not import `better-auth` or `@better-auth/*` outside the auth factory. Move package imports into `src/server/auth/better-auth.ts`, export `createAuth(...)` from that file, and have routes/handlers import the project factory or its `Auth` type instead. This keeps Better Auth construction behind request-time Worker `env`, database, and execution-context dependencies."
}

fn better_auth_config_help() -> &'static str {
    "Make `better-auth.config.ts` a Better Auth CLI schema-generation shim, not a runtime singleton re-export. Import `createAuth` from `./src/server/auth/better-auth`, create any placeholder DB/input required by your factory, then `export const auth = createAuth(..., {} as unknown as Env, {} as unknown as ExecutionContext)`. The config file may read Node/build-time values needed for schema generation, but it must not re-export a Worker runtime `auth` object."
}

fn better_auth_factory_help() -> &'static str {
    "In `src/server/auth/better-auth.ts`, export only `function createAuth(...)` and `type Auth = ReturnType<typeof createAuth>`. Do not export `const auth = betterAuth(...)` or other static config. Cloudflare Workers do not have request `env`, DB bindings, or `ExecutionContext` available at module load time, so Better Auth must be constructed per request with dependencies passed into `createAuth`."
}

fn is_allowed_auth_factory(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| rel_path == join(root, "src/server/auth/better-auth.ts"))
}

fn is_better_auth_config(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| rel_path == join(root, "better-auth.config.ts"))
}

fn is_allowed_session_cookie_adapter(rel_path: &str, source: &str, config: &Config) -> bool {
    source == "better-auth/cookies"
        && config
            .web_apps
            .iter()
            .any(|root| rel_path == join(root, "src/server/auth/session-cookie.ts"))
}

fn imports_better_auth_package(source: &str) -> bool {
    source == "better-auth"
        || source.starts_with("better-auth/")
        || source.starts_with("@better-auth/")
}

fn compact_code(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
}

#[derive(Debug)]
struct ExportName {
    name: String,
    is_type: bool,
}

fn parse_export_list(line: &str) -> Vec<ExportName> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return Vec::new();
    }

    let Some(rest) = trimmed.strip_prefix("export {") else {
        return Vec::new();
    };
    let Some((inside, _)) = rest.split_once('}') else {
        return Vec::new();
    };

    inside
        .split(',')
        .filter_map(|entry| {
            let mut entry = entry.trim();
            let is_type = if let Some(rest) = entry.strip_prefix("type ") {
                entry = rest.trim_start();
                true
            } else {
                false
            };
            let name = entry.split_whitespace().next().unwrap_or_default().trim();
            if name.is_empty() {
                None
            } else {
                Some(ExportName {
                    name: name.to_string(),
                    is_type,
                })
            }
        })
        .collect()
}
