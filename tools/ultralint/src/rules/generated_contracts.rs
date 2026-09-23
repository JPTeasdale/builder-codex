use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::join;
use crate::rules::{Report, Rule};
use crate::structured::parse_json;

pub struct GeneratedContractsRule;

impl Rule for GeneratedContractsRule {
    fn id(&self) -> &'static str {
        "generated-contracts"
    }

    fn category(&self) -> &'static str {
        "contracts"
    }

    fn description(&self) -> &'static str {
        "ensures generated DB, auth, router, and Worker artifacts exist"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.web_apps {
            check_web_contracts(project, app_root, report, self.id());
        }

        for app_root in &config.worker_apps {
            if project.exists(&join(app_root, "wrangler.jsonc"))
                && !project.exists(&join(app_root, "worker-configuration.d.ts"))
            {
                report.warning(
                    self.id(),
                    join(app_root, "worker-configuration.d.ts"),
                    None,
                    "Cloudflare Worker binding types are missing",
                    "run `pnpm gen:cf` after changing Wrangler bindings",
                );
            }
        }
    }
}

fn check_web_contracts(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    if project.exists(&join(app_root, "better-auth.config.ts")) {
        check_auth_contract(project, app_root, report, rule_id);
    }

    if project.exists(&join(app_root, "drizzle.config.ts")) {
        let drizzle_dir = project.root.join(join(app_root, "drizzle"));
        let has_migration = drizzle_dir.exists()
            && std::fs::read_dir(drizzle_dir)
                .map(|entries| {
                    entries.filter_map(Result::ok).any(|entry| {
                        entry
                            .path()
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext == "sql")
                    })
                })
                .unwrap_or(false);
        if !has_migration {
            report.warning(
                rule_id,
                join(app_root, "drizzle"),
                None,
                "no Drizzle SQL migrations were found",
                "run `pnpm db:generate` after defining the initial schema",
            );
        }
    }

    let has_routes = project.any_file_under(&join(app_root, "src/routes/"));
    if has_routes && !project.exists(&join(app_root, "src/routeTree.gen.ts")) {
        report.warning(
            rule_id,
            join(app_root, "src/routeTree.gen.ts"),
            None,
            "TanStack route tree output is missing",
            "run `pnpm gen:routes` if the project does not generate routes during dev/build",
        );
    }
}

fn check_auth_contract(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let package_path = join(app_root, "package.json");
    let script = project.read(&package_path).and_then(gen_auth_script);
    let Some(script) = script else {
        report.error(
            rule_id,
            package_path,
            None,
            "gen:auth does not identify a generated auth schema output",
            "Add a `gen:auth` script that runs the Better Auth generator through `pnpm exec` and passes `--output ./src/server/db/schema/auth.ts`, then run `pnpm gen:auth` and `pnpm db:generate`.",
        );
        return;
    };
    let Some(target) = auth_output_target(&script) else {
        report.error(
            rule_id,
            package_path,
            None,
            "gen:auth does not target auth.ts or auth.gen.ts",
            "Pass `--output ./src/server/db/schema/auth.ts` (or auth.gen.ts) to the Better Auth generator, invoke it with `pnpm gen:auth`, then run `pnpm db:generate`.",
        );
        return;
    };

    let target_path = join(app_root, &target);
    let Some(generated) = project.read(&target_path) else {
        report.error(
            rule_id,
            &target_path,
            None,
            format!("Better Auth schema targeted by gen:auth is missing: {target}"),
            "Run `pnpm gen:auth`, inspect the generated schema, then run `pnpm db:generate`.",
        );
        return;
    };

    if !has_generated_marker(generated) && !has_better_auth_cli_provenance(&script) {
        report.error(
            rule_id,
            target_path,
            None,
            "auth schema has no generated marker or Better Auth generator provenance",
            "Generate this exact file with the Better Auth CLI via `pnpm gen:auth`, or have the generator add an `@generated`/do-not-edit marker before running `pnpm db:generate`. Hand-written auth.ts files are not generated contracts.",
        );
    }
}

fn gen_auth_script(package_json: &str) -> Option<String> {
    parse_json(package_json)
        .ok()?
        .get("scripts")?
        .get("gen:auth")?
        .as_str()
        .map(str::to_string)
}

fn auth_output_target(script: &str) -> Option<String> {
    let words = shell_words(script);
    for (index, word) in words.iter().enumerate() {
        let candidate = if let Some(value) = word.strip_prefix("--output=") {
            Some(value)
        } else if matches!(word.as_str(), "--output" | "-o" | ">") {
            words.get(index + 1).map(String::as_str)
        } else {
            None
        };
        let Some(candidate) = candidate else {
            continue;
        };
        let normalized = candidate.trim_start_matches("./");
        if matches!(
            normalized,
            "src/server/db/schema/auth.ts" | "src/server/db/schema/auth.gen.ts"
        ) {
            return Some(normalized.to_string());
        }
    }
    None
}

fn shell_words(script: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for ch in script.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if ch == active {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn has_generated_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("@generated")
        || lower.contains("generated by better auth")
        || lower.contains("generated by better-auth")
        || lower.contains("do not edit")
}

fn has_better_auth_cli_provenance(script: &str) -> bool {
    let lower = script.to_ascii_lowercase();
    lower.contains("@better-auth/cli") && lower.contains("generate")
}

#[cfg(test)]
mod tests {
    use super::{auth_output_target, has_better_auth_cli_provenance};

    #[test]
    fn auth_ts_only_counts_when_the_generator_targets_it() {
        assert_eq!(
            auth_output_target(
                "pnpm exec @better-auth/cli generate --output ./src/server/db/schema/auth.ts"
            ),
            Some("src/server/db/schema/auth.ts".to_string())
        );
        assert_eq!(
            auth_output_target("pnpm exec @better-auth/cli generate --output ./auth.ts"),
            None
        );
    }

    #[test]
    fn recognizes_official_cli_provenance() {
        assert!(has_better_auth_cli_provenance(
            "pnpm exec @better-auth/cli generate -o src/server/db/schema/auth.gen.ts"
        ));
    }
}
