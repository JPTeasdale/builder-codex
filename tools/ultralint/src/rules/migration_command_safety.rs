use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as JsonValue;
use serde_yaml_ng::Value as YamlValue;

use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::{Report, Rule};

pub struct MigrationCommandSafetyRule;

impl Rule for MigrationCommandSafetyRule {
    fn id(&self) -> &'static str {
        "migration-command-safety"
    }

    fn category(&self) -> &'static str {
        "database"
    }

    fn description(&self) -> &'static str {
        "keeps deployment and canonical database commands on reviewed generated migrations instead of schema push or destructive reset paths"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        let packages = collect_package_scripts(project, report, self.id());
        for package in &packages {
            check_package_scripts(package, report, self.id());
        }

        let known_risks = known_script_risks(&packages);
        for file in &project.files {
            if !is_workflow_yaml(&file.rel_path) {
                continue;
            }
            check_workflow(file, &known_risks, report, self.id());
        }
    }
}

#[derive(Debug)]
struct PackageScripts {
    path: String,
    scripts: BTreeMap<String, String>,
    lines: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CommandRisk {
    push: bool,
    destructive: bool,
}

impl CommandRisk {
    fn merge(&mut self, other: Self) {
        self.push |= other.push;
        self.destructive |= other.destructive;
    }

    fn unsafe_for_deploy(self) -> bool {
        self.push || self.destructive
    }
}

fn collect_package_scripts(
    project: &Project,
    report: &mut Report,
    rule_id: &'static str,
) -> Vec<PackageScripts> {
    let mut packages = Vec::new();
    for file in &project.files {
        if file.rel_path.rsplit('/').next() != Some("package.json") {
            continue;
        }
        let package = match serde_json::from_str::<JsonValue>(&file.text) {
            Ok(package) => package,
            Err(error) => {
                report.error(
                    rule_id,
                    &file.rel_path,
                    None,
                    format!("cannot inspect migration commands because package.json is invalid: {error}"),
                    "Keep package.json as valid JSON so ultralint can inspect scripts structurally. Do not hide database commands in JSONC comments or malformed script entries.",
                );
                continue;
            }
        };
        let scripts = package
            .get("scripts")
            .and_then(JsonValue::as_object)
            .map(|scripts| {
                scripts
                    .iter()
                    .filter_map(|(name, command)| {
                        command
                            .as_str()
                            .map(|command| (name.clone(), command.to_string()))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let lines = scripts
            .keys()
            .filter_map(|name| {
                let needle = format!("\"{name}\"");
                file.text
                    .lines()
                    .position(|line| line.contains(&needle))
                    .map(|index| (name.clone(), index + 1))
            })
            .collect();
        packages.push(PackageScripts {
            path: file.rel_path.clone(),
            scripts,
            lines,
        });
    }
    packages
}

fn check_package_scripts(package: &PackageScripts, report: &mut Report, rule_id: &'static str) {
    for (name, command) in &package.scripts {
        let direct = direct_command_risk(command, Some(name));
        let expanded = expanded_script_risk(name, &package.scripts, &mut BTreeSet::new());

        if direct.destructive {
            report.error(
                rule_id,
                &package.path,
                script_line(package, name),
                format!(
                    "package script `{name}` contains a destructive database reset/forced push"
                ),
                migration_help(),
            );
            continue;
        }

        if direct.push {
            if is_explicitly_local_push(name) {
                report.warning(
                    rule_id,
                    &package.path,
                    script_line(package, name),
                    format!("local-only package script `{name}` uses drizzle-kit push"),
                    "Keep this command explicitly local-only and guard it against preview/production DATABASE_URL values. `drizzle-kit push` bypasses the reviewed migration artifact path; prefer `pnpm db:generate` plus a disposable local database even during development, and never call this script from CI, deploy, release, db:migrate, or db:init.",
                );
            } else {
                report.error(
                    rule_id,
                    &package.path,
                    script_line(package, name),
                    format!("package script `{name}` uses drizzle-kit push outside an explicit local-only command"),
                    migration_help(),
                );
            }
            continue;
        }

        if is_deploy_or_canonical_migration(name) && expanded.unsafe_for_deploy() {
            report.error(
                rule_id,
                &package.path,
                script_line(package, name),
                format!("canonical/deploy script `{name}` indirectly invokes schema push or reset"),
                migration_help(),
            );
        }
    }
}

fn check_workflow(
    file: &ProjectFile,
    known_risks: &BTreeMap<String, CommandRisk>,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Ok(workflow) = serde_yaml_ng::from_str::<YamlValue>(&file.text) else {
        return;
    };
    let mut run_commands = Vec::new();
    collect_run_commands(&workflow, &mut run_commands);

    for command in run_commands {
        let mut risk = direct_command_risk(&command, None);
        for script in referenced_scripts(&command, known_risks.keys()) {
            if let Some(script_risk) = known_risks.get(&script) {
                risk.merge(*script_risk);
            }
        }
        if !risk.unsafe_for_deploy() {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            workflow_command_line(&file.text, &command),
            "workflow run command invokes schema push, forced push, or destructive database reset",
            "CI and deployment workflows must apply reviewed generated migrations only. Replace push/reset with `pnpm db:generate` during development, commit and review the SQL, validate with `pnpm db:check`, then run `pnpm db:migrate` against the explicitly selected branch/environment. A script named local is still forbidden from workflows.",
        );
    }
}

fn known_script_risks(packages: &[PackageScripts]) -> BTreeMap<String, CommandRisk> {
    let mut risks = BTreeMap::<String, CommandRisk>::new();
    for package in packages {
        for name in package.scripts.keys() {
            let risk = expanded_script_risk(name, &package.scripts, &mut BTreeSet::new());
            risks.entry(name.clone()).or_default().merge(risk);
        }
    }
    risks
}

fn expanded_script_risk(
    name: &str,
    scripts: &BTreeMap<String, String>,
    visiting: &mut BTreeSet<String>,
) -> CommandRisk {
    if !visiting.insert(name.to_string()) {
        return CommandRisk::default();
    }
    let Some(command) = scripts.get(name) else {
        visiting.remove(name);
        return CommandRisk::default();
    };

    let mut risk = direct_command_risk(command, Some(name));
    for referenced in referenced_scripts(command, scripts.keys()) {
        risk.merge(expanded_script_risk(&referenced, scripts, visiting));
    }
    visiting.remove(name);
    risk
}

fn direct_command_risk(command: &str, script_name: Option<&str>) -> CommandRisk {
    let lower = normalize_command(command);
    let tokens = command_tokens(&lower);
    let drizzle_push = tokens
        .windows(2)
        .any(|window| window[0] == "drizzle-kit" && window[1] == "push");
    let named_push = tokens.iter().any(|token| {
        token == "db:push"
            || token.starts_with("db:push:")
            || (token.starts_with("db:") && token.ends_with(":push"))
    });
    let push = drizzle_push || named_push;
    let forced_push = push && tokens.iter().any(|token| token == "--force");
    let destructive_text = [
        "drizzle-kit drop",
        "prisma migrate reset",
        "drop database",
        "drop schema",
        "truncate table",
        "database reset",
        "migrate reset",
        "migration reset",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern));
    let reset_token = tokens
        .iter()
        .any(|token| token == "db:reset" || token.starts_with("db:reset:"));
    let reset_script = script_name.is_some_and(|name| {
        let name = name.to_ascii_lowercase();
        name == "db:reset"
            || name.starts_with("db:reset:")
            || name.starts_with("db:") && name.ends_with(":reset")
    });

    CommandRisk {
        push,
        destructive: forced_push || destructive_text || reset_token || reset_script,
    }
}

fn is_explicitly_local_push(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let parts = lower
        .split([':', '-', '_'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    parts.contains(&"push") && parts.contains(&"local")
}

fn is_deploy_or_canonical_migration(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "deploy"
        || lower == "release"
        || lower == "migrate"
        || lower == "migration"
        || lower.contains("deploy")
        || lower.contains("release")
        || lower.starts_with("db:migrate")
        || lower.starts_with("db:migration")
        || lower.starts_with("db:apply")
        || lower.starts_with("db:init")
        || lower.starts_with("db:prod")
        || lower.starts_with("db:production")
}

fn referenced_scripts<'a>(
    command: &str,
    script_names: impl Iterator<Item = &'a String>,
) -> Vec<String> {
    let normalized = normalize_command(command);
    script_names
        .filter(|name| {
            [
                format!("pnpm run {name}"),
                format!("pnpm {name}"),
                format!("npm run {name}"),
                format!("yarn run {name}"),
                format!("yarn {name}"),
                format!("bun run {name}"),
                format!("bun {name}"),
            ]
            .iter()
            .any(|invocation| contains_command_phrase(&normalized, invocation))
        })
        .cloned()
        .collect()
}

fn contains_command_phrase(command: &str, phrase: &str) -> bool {
    command.match_indices(phrase).any(|(index, _)| {
        let before = command[..index].chars().next_back();
        let after = command[index + phrase.len()..].chars().next();
        before.is_none_or(is_shell_boundary) && after.is_none_or(is_shell_boundary)
    })
}

fn is_shell_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '&' | '|' | ';' | '(' | ')' | '"' | '\'')
}

fn normalize_command(command: &str) -> String {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn command_tokens(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|ch: char| {
                    matches!(ch, '&' | '|' | ';' | '(' | ')' | '"' | '\'' | '`' | ',')
                })
                .to_string()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn collect_run_commands(value: &YamlValue, commands: &mut Vec<String>) {
    match value {
        YamlValue::Mapping(mapping) => {
            for (key, value) in mapping {
                if key.as_str() == Some("run") {
                    if let Some(command) = value.as_str() {
                        commands.push(command.to_string());
                    }
                } else {
                    collect_run_commands(value, commands);
                }
            }
        }
        YamlValue::Sequence(sequence) => {
            for value in sequence {
                collect_run_commands(value, commands);
            }
        }
        _ => {}
    }
}

fn is_workflow_yaml(rel_path: &str) -> bool {
    rel_path.starts_with(".github/workflows/")
        && (rel_path.ends_with(".yml") || rel_path.ends_with(".yaml"))
}

fn script_line(package: &PackageScripts, name: &str) -> Option<usize> {
    package.lines.get(name).copied()
}

fn workflow_command_line(text: &str, command: &str) -> Option<usize> {
    let first_line = command.lines().find(|line| !line.trim().is_empty())?.trim();
    text.lines()
        .position(|line| line.contains(first_line))
        .map(|index| index + 1)
}

fn migration_help() -> &'static str {
    "Canonical migration and deploy commands must use reviewed generated migrations: `pnpm db:generate`, inspect and commit the SQL/snapshot, run `pnpm db:check`, then apply with `pnpm db:migrate` to the explicitly selected database branch. Remove `drizzle-kit push`, `push --force`, reset, DROP DATABASE/SCHEMA, and TRUNCATE paths from deploy/release/database scripts. If push is retained for disposable local development, isolate it under an explicitly local name such as `db:push:local`, add a local-URL guard, and never reference it from CI or deployment scripts."
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        direct_command_risk, expanded_script_risk, is_explicitly_local_push, referenced_scripts,
    };

    #[test]
    fn detects_push_force_and_reset_commands() {
        assert!(direct_command_risk("drizzle-kit push", None).push);
        assert!(direct_command_risk("drizzle-kit push --force", None).destructive);
        assert!(direct_command_risk("prisma migrate reset", None).destructive);
    }

    #[test]
    fn resolves_push_through_package_script_aliases() {
        let scripts = BTreeMap::from([
            ("db:push:local".to_string(), "drizzle-kit push".to_string()),
            (
                "deploy".to_string(),
                "pnpm db:push:local && wrangler deploy".to_string(),
            ),
        ]);
        let risk = expanded_script_risk("deploy", &scripts, &mut BTreeSet::new());
        assert!(risk.push);
        assert_eq!(
            referenced_scripts("pnpm db:push:local", scripts.keys()),
            ["db:push:local"]
        );
    }

    #[test]
    fn only_explicit_local_push_names_are_warning_eligible() {
        assert!(is_explicitly_local_push("db:push:local"));
        assert!(!is_explicitly_local_push("db:push"));
    }
}
