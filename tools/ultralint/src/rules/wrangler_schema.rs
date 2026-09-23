use std::fs;

use serde_json::Value;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};
use crate::structured::parse_jsonc;

pub struct WranglerSchemaRule;

impl Rule for WranglerSchemaRule {
    fn id(&self) -> &'static str {
        "wrangler-schema"
    }

    fn category(&self) -> &'static str {
        "cloudflare"
    }

    fn description(&self) -> &'static str {
        "validates wrangler.jsonc against node_modules/wrangler/config-schema.json"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.worker_apps {
            let wrangler_path = join(app_root, "wrangler.jsonc");
            if !project.exists(&wrangler_path) {
                continue;
            }

            let Some(wrangler) = read_jsonc(project, &wrangler_path, report, self.id()) else {
                continue;
            };

            check_schema_property(&wrangler, &wrangler_path, report, self.id());
            validate_against_schema(
                project,
                app_root,
                &wrangler_path,
                &wrangler,
                report,
                self.id(),
            );
        }
    }
}

fn check_schema_property(
    wrangler: &Value,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let actual = wrangler.get("$schema").and_then(Value::as_str);
    if actual == Some(WRANGLER_SCHEMA_PATH) {
        return;
    }

    let found = actual
        .map(|value| format!("found `{value}`"))
        .unwrap_or_else(|| "missing".to_string());
    report.error(
        rule_id,
        wrangler_path,
        None,
        format!("wrangler.jsonc $schema must be `{WRANGLER_SCHEMA_PATH}` ({found})"),
        "add `\"$schema\": \"node_modules/wrangler/config-schema.json\"` so editors and ultralint use the same Wrangler schema",
    );
}

fn validate_against_schema(
    project: &Project,
    app_root: &str,
    wrangler_path: &str,
    wrangler: &Value,
    report: &mut Report,
    rule_id: &'static str,
) {
    let schema_path = join(app_root, WRANGLER_SCHEMA_PATH);
    let schema_file = project.root.join(&schema_path);
    if !schema_file.exists() {
        report.error(
            rule_id,
            &schema_path,
            None,
            "Wrangler config schema is missing",
            "Install dependencies from the app root and ensure Wrangler is present, for example `pnpm install` or `pnpm add -D wrangler`, so node_modules/wrangler/config-schema.json is available.",
        );
        return;
    }

    let schema_text = match fs::read_to_string(&schema_file) {
        Ok(text) => text,
        Err(err) => {
            report.error(
                rule_id,
                &schema_path,
                None,
                format!("could not read Wrangler config schema: {err}"),
                "make sure node_modules/wrangler/config-schema.json is readable",
            );
            return;
        }
    };
    let schema = match serde_json::from_str::<Value>(&schema_text) {
        Ok(schema) => schema,
        Err(err) => {
            report.error(
                rule_id,
                &schema_path,
                None,
                format!("could not parse Wrangler config schema: {err}"),
                "reinstall Wrangler if node_modules/wrangler/config-schema.json is invalid",
            );
            return;
        }
    };
    let validator = match jsonschema::validator_for(&schema) {
        Ok(validator) => validator,
        Err(err) => {
            report.error(
                rule_id,
                &schema_path,
                None,
                format!("could not compile Wrangler config schema: {err}"),
                "reinstall Wrangler or update ultralint if the schema dialect changed",
            );
            return;
        }
    };

    for error in validator.iter_errors(wrangler).take(5) {
        report.error(
            rule_id,
            wrangler_path,
            None,
            format!(
                "wrangler.jsonc does not match Wrangler schema at {}: {}",
                error.instance_path(),
                error
            ),
            "fix wrangler.jsonc to match node_modules/wrangler/config-schema.json",
        );
    }
}

fn read_jsonc(
    project: &Project,
    rel_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) -> Option<Value> {
    let text = match fs::read_to_string(project.root.join(rel_path)) {
        Ok(text) => text,
        Err(err) => {
            report.error(
                rule_id,
                rel_path,
                None,
                format!("could not read {rel_path}: {err}"),
                "make sure Wrangler config is readable by repository tools",
            );
            return None;
        }
    };

    match parse_jsonc(&text) {
        Ok(value) => Some(value),
        Err(err) => {
            report.error(
                rule_id,
                rel_path,
                None,
                format!("could not parse {rel_path}: {err}"),
                "keep wrangler.jsonc valid JSONC before schema validation runs",
            );
            None
        }
    }
}

fn join(root: &str, rel_path: &str) -> String {
    if root == "." || root.is_empty() {
        rel_path.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel_path)
    }
}

const WRANGLER_SCHEMA_PATH: &str = "node_modules/wrangler/config-schema.json";
