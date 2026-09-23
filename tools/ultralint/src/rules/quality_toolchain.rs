use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::config::Config;
use crate::fs::{Project, normalize};
use crate::rules::common::join;
use crate::rules::{Report, Rule};
use crate::structured::{parse_json, parse_jsonc};

pub struct QualityToolchainRule;

impl Rule for QualityToolchainRule {
    fn id(&self) -> &'static str {
        "quality-toolchain"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "requires a pinned pnpm, TypeScript, test, lint, and build toolchain"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        check_root_package_manager(project, report, self.id());
        check_lockfiles(project, report, self.id());

        for package_root in package_roots(project, config) {
            check_package_scripts(project, &package_root, report, self.id());
            check_tsconfig(project, &package_root, report, self.id());
        }
        check_biome(project, report, self.id());
    }
}

fn check_root_package_manager(project: &Project, report: &mut Report, rule_id: &'static str) {
    let Some(text) = project.read("package.json") else {
        report.error(
            rule_id,
            "package.json",
            None,
            "root package.json is required for the quality toolchain",
            "create the root package with `pnpm init` and commit its pinned `packageManager` field",
        );
        return;
    };
    let Ok(package) = parse_json(text) else {
        report.error(
            rule_id,
            "package.json",
            None,
            "root package.json is not valid JSON",
            "fix package.json before running `pnpm check`",
        );
        return;
    };
    let actual = package.get("packageManager").and_then(Value::as_str);
    if actual.is_some_and(is_pinned_pnpm) {
        return;
    }
    report.error(
        rule_id,
        "package.json",
        None,
        format!(
            "packageManager must pin an exact pnpm version (found {})",
            actual.unwrap_or("missing")
        ),
        "set `packageManager` to the exact installed version, for example `pnpm@10.13.1`; confirm it with `pnpm --version`",
    );
}

fn is_pinned_pnpm(value: &str) -> bool {
    let Some(version) = value.strip_prefix("pnpm@") else {
        return false;
    };
    let version = version.split('+').next().unwrap_or(version);
    let core = version.split('-').next().unwrap_or(version);
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn check_lockfiles(project: &Project, report: &mut Report, rule_id: &'static str) {
    if !project.is_file("pnpm-lock.yaml") {
        report.error(
            rule_id,
            "pnpm-lock.yaml",
            None,
            "pnpm-lock.yaml is required",
            "generate and commit the lockfile with `pnpm install --lockfile-only`",
        );
    }

    for path in find_competing_lockfiles(&project.root) {
        report.error(
            rule_id,
            path,
            None,
            "competing package-manager lockfile is not allowed in a pnpm repository",
            "remove the competing lockfile and refresh pnpm-lock.yaml with `pnpm install --lockfile-only`",
        );
    }
}

fn find_competing_lockfiles(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !matches!(
                    name.as_ref(),
                    ".git" | ".wrangler" | "node_modules" | "dist" | "build" | "coverage"
                ) {
                    walk(root, &path, found);
                }
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if COMPETING_LOCKFILES.contains(&name) {
                found.push(normalize(path.strip_prefix(root).unwrap_or(&path)));
            }
        }
    }

    let mut found = Vec::new();
    walk(root, root, &mut found);
    found.sort();
    found
}

fn package_roots(project: &Project, config: &Config) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();
    if project.is_file("package.json") {
        roots.insert(".".to_string());
    }
    for root in config
        .web_apps
        .iter()
        .chain(&config.worker_apps)
        .chain(&config.mobile_apps)
    {
        if project.is_file(&join(root, "package.json")) {
            roots.insert(root.clone());
        }
    }
    roots
}

fn check_package_scripts(
    project: &Project,
    package_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let path = join(package_root, "package.json");
    let Some(text) = project.read(&path) else {
        return;
    };
    let package = match parse_json(text) {
        Ok(package) => package,
        Err(error) => {
            report.error(
                rule_id,
                &path,
                None,
                format!("could not parse package.json: {error}"),
                "fix package.json before running `pnpm check`",
            );
            return;
        }
    };
    let scripts = package.get("scripts").and_then(Value::as_object);
    let missing = REQUIRED_SCRIPTS
        .iter()
        .filter(|script| {
            scripts
                .and_then(|scripts| scripts.get(**script))
                .and_then(Value::as_str)
                .is_none_or(|command| command.trim().is_empty())
        })
        .copied()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return;
    }
    report.error(
        rule_id,
        path,
        None,
        format!("package scripts are missing: {}", missing.join(", ")),
        "add non-empty `ultralint`, `check`, `lint`, `test`, and `build` scripts; each must be runnable as `pnpm <script>`",
    );
}

fn check_tsconfig(
    project: &Project,
    package_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let path = join(package_root, "tsconfig.json");
    let Some(text) = project.read(&path) else {
        report.error(
            rule_id,
            path,
            None,
            "tsconfig.json is required",
            "add a strict TypeScript config and verify it with `pnpm check`",
        );
        return;
    };
    let tsconfig = match parse_jsonc(text) {
        Ok(tsconfig) => tsconfig,
        Err(error) => {
            report.error(
                rule_id,
                &path,
                None,
                format!("could not parse tsconfig.json: {error}"),
                "fix tsconfig.json JSONC before running `pnpm check`",
            );
            return;
        }
    };
    let compiler = tsconfig.get("compilerOptions").and_then(Value::as_object);
    if compiler
        .and_then(|options| options.get("strict"))
        .and_then(Value::as_bool)
        != Some(true)
    {
        report.error(
            rule_id,
            &path,
            None,
            "compilerOptions.strict must be true",
            "set `compilerOptions.strict` to true and resolve resulting errors with `pnpm check`",
        );
    }

    let disabled = STRICT_FAMILY_OPTIONS
        .iter()
        .filter(|option| {
            compiler
                .and_then(|options| options.get(**option))
                .and_then(Value::as_bool)
                == Some(false)
        })
        .copied()
        .collect::<Vec<_>>();
    if !disabled.is_empty() {
        report.error(
            rule_id,
            path,
            None,
            format!(
                "strict-family compiler options must not be disabled: {}",
                disabled.join(", ")
            ),
            "remove the false overrides, keep `strict: true`, and verify the project with `pnpm check`",
        );
    }
}

fn check_biome(project: &Project, report: &mut Report, rule_id: &'static str) {
    let path = ["biome.json", "biome.jsonc"]
        .into_iter()
        .find(|path| project.is_file(path));
    let Some(path) = path else {
        report.error(
            rule_id,
            "biome.jsonc",
            None,
            "Biome configuration is required",
            "create biome.jsonc with the recommended linter rules, then verify it with `pnpm exec biome check .`",
        );
        return;
    };
    let Some(text) = project.read(path) else {
        return;
    };
    let biome = match parse_jsonc(text) {
        Ok(biome) => biome,
        Err(error) => {
            report.error(
                rule_id,
                path,
                None,
                format!("could not parse Biome configuration: {error}"),
                "fix the Biome JSONC configuration before running `pnpm lint`",
            );
            return;
        }
    };
    let linter_enabled = nested_value(&biome, &["linter", "enabled"]).and_then(Value::as_bool);
    let recommended =
        nested_value(&biome, &["linter", "rules", "recommended"]).and_then(Value::as_bool);
    let recommended_preset =
        nested_value(&biome, &["linter", "rules", "preset"]).and_then(Value::as_str);
    if linter_enabled == Some(true)
        && (recommended == Some(true) || recommended_preset == Some("recommended"))
    {
        return;
    }
    report.error(
        rule_id,
        path,
        None,
        "Biome must enable the linter and recommended rules",
        "set `linter.enabled` to true and use either `linter.rules.preset: \"recommended\"` (Biome 2) or `linter.rules.recommended: true`, then run `pnpm lint`",
    );
}

fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

const REQUIRED_SCRIPTS: &[&str] = &["ultralint", "check", "lint", "test", "build"];

const STRICT_FAMILY_OPTIONS: &[&str] = &[
    "alwaysStrict",
    "noImplicitAny",
    "noImplicitThis",
    "strictBindCallApply",
    "strictBuiltinIteratorReturn",
    "strictFunctionTypes",
    "strictNullChecks",
    "strictPropertyInitialization",
    "useUnknownInCatchVariables",
];

const COMPETING_LOCKFILES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
];

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{is_pinned_pnpm, nested_value};

    #[test]
    fn package_manager_requires_an_exact_pnpm_semver() {
        assert!(is_pinned_pnpm("pnpm@10.13.1"));
        assert!(is_pinned_pnpm("pnpm@10.13.1+sha512.deadbeef"));
        assert!(!is_pinned_pnpm("pnpm@latest"));
        assert!(!is_pinned_pnpm("pnpm@^10.13.1"));
        assert!(!is_pinned_pnpm("npm@11.0.0"));
    }

    #[test]
    fn recommended_biome_path_is_structured() {
        let biome = json!({ "linter": { "rules": { "recommended": true } } });
        assert_eq!(
            nested_value(&biome, &["linter", "rules", "recommended"]),
            Some(&json!(true))
        );
        let biome = json!({ "linter": { "rules": { "preset": "recommended" } } });
        assert_eq!(
            nested_value(&biome, &["linter", "rules", "preset"]),
            Some(&json!("recommended"))
        );
    }
}
