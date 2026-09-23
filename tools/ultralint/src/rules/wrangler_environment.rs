use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};
use crate::structured::parse_jsonc;

pub struct WranglerEnvSurfaceRule;

impl Rule for WranglerEnvSurfaceRule {
    fn id(&self) -> &'static str {
        "wrangler-env-surface"
    }

    fn category(&self) -> &'static str {
        "cloudflare"
    }

    fn description(&self) -> &'static str {
        "ensures Wrangler environments expose the required vars and the same binding surface"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.worker_apps {
            let wrangler_path = join(app_root, "wrangler.jsonc");
            let Some(wrangler) = read_wrangler(project, &wrangler_path, report, self.id()) else {
                continue;
            };

            check_required_wrangler_envs(&wrangler, &wrangler_path, report, self.id());
            check_app_env_values(&wrangler, &wrangler_path, report, self.id());
            check_secret_shapes(&wrangler, &wrangler_path, report, self.id());
            check_empty_required_secrets(&wrangler, &wrangler_path, report, self.id());
            check_production_placeholders(&wrangler, &wrangler_path, report, self.id());
            check_hyperdrive_local_connection_strings(&wrangler, &wrangler_path, report, self.id());
            check_secret_docs_sync(
                project,
                app_root,
                &collect_all_wrangler_secrets(&wrangler),
                report,
                self.id(),
            );

            let surfaces = collect_environment_surfaces(&wrangler);
            if surfaces.len() < 2 {
                continue;
            }

            let baseline = &surfaces[0];
            for surface in surfaces.iter().skip(1) {
                let expected = expected_surface(&baseline.items, &surface.name);
                let missing = difference(&expected, &surface.items);
                let extra = difference(&surface.items, &expected);
                if missing.is_empty() && extra.is_empty() {
                    continue;
                }

                let mut details = Vec::new();
                if !missing.is_empty() {
                    details.push(format!("missing {}", missing.join(", ")));
                }
                if !extra.is_empty() {
                    details.push(format!("extra {}", extra.join(", ")));
                }

                let mut help = "repeat the same non-`DEV_*` `vars` keys, `secrets.required` names, and binding names in every environment; include each `DEV_*` var in top-level and every non-production environment, but omit it from `env.production`; values and resource IDs may differ".to_string();
                if surface.name == "env.production"
                    && missing.iter().any(|item| is_non_dev_var(item))
                {
                    help.push_str(". If a variable should not be set in production, prefix it with `DEV_` in top-level and every non-production environment");
                }

                report.error(
                    self.id(),
                    &wrangler_path,
                    None,
                    format!(
                        "Wrangler environment `{}` does not match `{}` binding/var surface: {}",
                        surface.name,
                        baseline.name,
                        details.join("; ")
                    ),
                    help,
                );
            }
        }
    }
}

pub struct EnvironmentFilesRule;

impl Rule for EnvironmentFilesRule {
    fn id(&self) -> &'static str {
        "environment-files"
    }

    fn category(&self) -> &'static str {
        "cloudflare"
    }

    fn description(&self) -> &'static str {
        "keeps .dev.vars.example aligned to Wrangler vars and .env.example reserved for build/dev variables"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.worker_apps {
            let wrangler_path = join(app_root, "wrangler.jsonc");
            let Some(wrangler) = read_wrangler(project, &wrangler_path, report, self.id()) else {
                continue;
            };

            let wrangler_runtime_names = collect_all_wrangler_runtime_names(&wrangler);
            let hyperdrive_overrides = collect_hyperdrive_local_overrides(&wrangler);
            let build_dev_vars = collect_build_dev_vars(
                project,
                app_root,
                &wrangler_runtime_names,
                &hyperdrive_overrides,
            );

            check_dev_vars_example(
                project,
                app_root,
                &wrangler_runtime_names,
                report,
                self.id(),
            );
            check_env_example(
                project,
                app_root,
                &wrangler_runtime_names,
                &build_dev_vars,
                report,
                self.id(),
            );
        }
    }
}

#[derive(Debug)]
struct EnvSurface {
    name: String,
    items: BTreeSet<String>,
}

fn read_wrangler(
    project: &Project,
    rel_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) -> Option<Value> {
    if !project.exists(rel_path) {
        return None;
    }
    let path = project.root.join(rel_path);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            report.error(
                rule_id,
                rel_path,
                None,
                format!("could not read {rel_path}: {err}"),
                "make sure Wrangler config is readable by the repository tools",
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
                "keep wrangler.jsonc valid JSONC so ultralint can compare environment surfaces",
            );
            None
        }
    }
}

fn collect_environment_surfaces(wrangler: &Value) -> Vec<EnvSurface> {
    let mut surfaces = Vec::new();
    let top_level = collect_surface(wrangler);
    if !top_level.is_empty() {
        surfaces.push(EnvSurface {
            name: "top-level".to_string(),
            items: top_level,
        });
    }

    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for (name, env) in envs {
            surfaces.push(EnvSurface {
                name: format!("env.{name}"),
                items: collect_surface(env),
            });
        }
    }

    surfaces
}

fn collect_surface(config: &Value) -> BTreeSet<String> {
    let mut items = BTreeSet::new();

    for key in collect_vars(config) {
        items.insert(format!("vars.{key}"));
    }

    for secret in collect_secrets(config) {
        items.insert(format!("secrets.{secret}"));
    }

    for &(section, field) in ARRAY_BINDING_SECTIONS {
        collect_array_bindings(config, section, field, &mut items);
    }

    for section in OBJECT_BINDING_SECTIONS {
        if let Some(binding) = config
            .get(section)
            .and_then(|value| value.get("binding"))
            .and_then(Value::as_str)
        {
            items.insert(format!("{section}.{binding}"));
        }
    }

    collect_nested_array_bindings(
        config,
        &["durable_objects", "bindings"],
        "durable_objects",
        "name",
        &mut items,
    );
    collect_nested_array_bindings(
        config,
        &["queues", "producers"],
        "queues.producers",
        "binding",
        &mut items,
    );
    collect_nested_array_bindings(
        config,
        &["unsafe", "bindings"],
        "unsafe",
        "name",
        &mut items,
    );

    for section in OBJECT_KEY_BINDING_SECTIONS {
        if let Some(object) = config.get(section).and_then(Value::as_object) {
            for key in object.keys() {
                items.insert(format!("{section}.{key}"));
            }
        }
    }

    items
}

fn expected_surface(baseline: &BTreeSet<String>, environment_name: &str) -> BTreeSet<String> {
    if environment_name != "env.production" {
        return baseline.clone();
    }

    baseline
        .iter()
        .filter(|item| !is_dev_var(item))
        .cloned()
        .collect()
}

fn is_dev_var(item: &str) -> bool {
    item.strip_prefix("vars.")
        .is_some_and(|name| name.starts_with("DEV_"))
}

fn is_non_dev_var(item: &str) -> bool {
    item.starts_with("vars.") && !is_dev_var(item)
}

fn collect_all_wrangler_vars(wrangler: &Value) -> BTreeSet<String> {
    let mut vars = collect_vars(wrangler);
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            vars.extend(collect_vars(env));
        }
    }
    vars
}

fn collect_all_wrangler_runtime_names(wrangler: &Value) -> BTreeSet<String> {
    let mut names = collect_all_wrangler_vars(wrangler);
    names.extend(collect_all_wrangler_secrets(wrangler));
    names
}

fn collect_all_wrangler_secrets(wrangler: &Value) -> BTreeSet<String> {
    let mut secrets = collect_secrets(wrangler);
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            secrets.extend(collect_secrets(env));
        }
    }
    secrets
}

pub(super) fn collect_worker_env_names(wrangler: &Value) -> BTreeSet<String> {
    let mut names = collect_worker_env_names_from_config(wrangler);
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            names.extend(collect_worker_env_names_from_config(env));
        }
    }
    names
}

fn collect_worker_env_names_from_config(config: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    names.extend(collect_vars(config));
    names.extend(collect_secrets(config));

    for &(section, field) in ARRAY_BINDING_SECTIONS {
        collect_array_binding_names(config, section, field, &mut names);
    }

    for section in OBJECT_BINDING_SECTIONS {
        if let Some(binding) = config
            .get(section)
            .and_then(|value| value.get("binding"))
            .and_then(Value::as_str)
        {
            names.insert(binding.to_string());
        }
    }

    collect_nested_array_binding_names(
        config,
        &["durable_objects", "bindings"],
        "name",
        &mut names,
    );
    collect_nested_array_binding_names(config, &["queues", "producers"], "binding", &mut names);
    collect_nested_array_binding_names(config, &["unsafe", "bindings"], "name", &mut names);

    for section in OBJECT_KEY_BINDING_SECTIONS {
        if let Some(object) = config.get(section).and_then(Value::as_object) {
            names.extend(object.keys().cloned());
        }
    }

    names
}

fn collect_vars(config: &Value) -> BTreeSet<String> {
    config
        .get("vars")
        .and_then(Value::as_object)
        .map(|vars| vars.keys().cloned().collect())
        .unwrap_or_default()
}

fn collect_secrets(config: &Value) -> BTreeSet<String> {
    config
        .get("secrets")
        .and_then(|secrets| secrets.get("required"))
        .and_then(Value::as_array)
        .map(|entries| collect_string_array(entries))
        .unwrap_or_default()
}

fn collect_string_array(entries: &[Value]) -> BTreeSet<String> {
    entries
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn collect_array_bindings(
    config: &Value,
    section: &str,
    field: &str,
    items: &mut BTreeSet<String>,
) {
    let Some(entries) = config.get(section).and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        if let Some(binding) = entry.get(field).and_then(Value::as_str) {
            items.insert(format!("{section}.{binding}"));
        }
    }
}

fn collect_array_binding_names(
    config: &Value,
    section: &str,
    field: &str,
    names: &mut BTreeSet<String>,
) {
    let Some(entries) = config.get(section).and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        if let Some(binding) = entry.get(field).and_then(Value::as_str) {
            names.insert(binding.to_string());
        }
    }
}

fn collect_nested_array_bindings(
    config: &Value,
    path: &[&str],
    label: &str,
    field: &str,
    items: &mut BTreeSet<String>,
) {
    let Some(entries) = nested_value(config, path).and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        if let Some(binding) = entry.get(field).and_then(Value::as_str) {
            items.insert(format!("{label}.{binding}"));
        }
    }
}

fn collect_nested_array_binding_names(
    config: &Value,
    path: &[&str],
    field: &str,
    names: &mut BTreeSet<String>,
) {
    let Some(entries) = nested_value(config, path).and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        if let Some(binding) = entry.get(field).and_then(Value::as_str) {
            names.insert(binding.to_string());
        }
    }
}

fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

fn check_secret_shapes(
    wrangler: &Value,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    check_config_secret_shape(wrangler, "top-level", wrangler_path, report, rule_id);

    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for (name, env) in envs {
            check_config_secret_shape(env, &format!("env.{name}"), wrangler_path, report, rule_id);
        }
    }
}

fn check_config_secret_shape(
    config: &Value,
    label: &str,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(secrets) = config.get("secrets") else {
        return;
    };

    if secrets
        .as_object()
        .and_then(|object| object.get("required"))
        .is_some_and(Value::is_array)
    {
        return;
    }

    report.error(
        rule_id,
        wrangler_path,
        None,
        format!("Wrangler `{label}` secrets must use `secrets.required`"),
        "use the documented Wrangler shape: `\"secrets\": { \"required\": [\"SECRET_NAME\"] }`",
    );
}

fn check_empty_required_secrets(
    wrangler: &Value,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    check_config_empty_required_secrets(wrangler, "top-level", wrangler_path, report, rule_id);

    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for (name, env) in envs {
            check_config_empty_required_secrets(
                env,
                &format!("env.{name}"),
                wrangler_path,
                report,
                rule_id,
            );
        }
    }
}

fn check_config_empty_required_secrets(
    config: &Value,
    label: &str,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(entries) = config
        .get("secrets")
        .and_then(|secrets| secrets.get("required"))
        .and_then(Value::as_array)
    else {
        return;
    };

    if !entries.iter().any(|entry| entry.as_str() == Some("")) {
        return;
    }

    report.error(
        rule_id,
        wrangler_path,
        None,
        format!("Wrangler `{label}` has an empty required secret name"),
        "Remove empty strings from `secrets.required`. Required secrets must be explicit names such as `BETTER_AUTH_SECRET`; then document the same name in .dev.vars.example.",
    );
}

fn check_production_placeholders(
    wrangler: &Value,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(production) = wrangler
        .get("env")
        .and_then(Value::as_object)
        .and_then(|envs| envs.get("production"))
    else {
        return;
    };

    let text = production.to_string();
    if !(text.contains("__PLACEHOLDER__")
        || text.contains("__PRODUCTION_REPLACE")
        || text.contains("00000000000000000000000000000000")
        || text.contains(":\"\"")
        || text.contains(": \"\""))
    {
        return;
    }

    report.error(
        rule_id,
        wrangler_path,
        None,
        "Wrangler production environment contains placeholder values",
        "Replace production placeholders before deploy. `env.production` must not contain `__PLACEHOLDER__`, all-zero Cloudflare resource IDs, or empty strings. Use preview placeholders only in env.preview.",
    );
}

fn check_secret_docs_sync(
    project: &Project,
    app_root: &str,
    secrets: &BTreeSet<String>,
    report: &mut Report,
    rule_id: &'static str,
) {
    if secrets.is_empty() {
        return;
    }

    let docs_path = join(app_root, "docs/SECRETS.md");
    let Some(docs) = project.read(&docs_path) else {
        report.error(
            rule_id,
            docs_path,
            None,
            "Wrangler required secrets are not documented",
            format!(
                "Create docs/SECRETS.md and list each `secrets.required` entry from wrangler.jsonc: {}. Include where the secret is set locally (.dev.vars for local development only), how to create/update it in Cloudflare (`pnpm exec wrangler secret put SECRET_NAME --env preview` and `pnpm exec wrangler secret put SECRET_NAME --env production`), and which deploy workflows require it.",
                format_set(secrets)
            ),
        );
        return;
    };

    let missing_from_docs = secrets
        .iter()
        .filter(|secret| !docs.contains(secret.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_from_docs.is_empty() {
        report.error(
            rule_id,
            docs_path,
            None,
            format!(
                "docs/SECRETS.md is missing Wrangler required secrets: {}",
                missing_from_docs.join(", ")
            ),
            "Keep docs/SECRETS.md in sync with wrangler.jsonc `secrets.required`. Each entry should say whether it is required for preview, production, local .dev.vars, and deploy automation.",
        );
    }

    let workflow_prefix = join(app_root, ".github/workflows/");
    for workflow in &project.files {
        if !workflow.rel_path.starts_with(&workflow_prefix)
            || !(workflow.rel_path.ends_with(".yml") || workflow.rel_path.ends_with(".yaml"))
            || !workflow_mentions_deploy(&workflow.text)
        {
            continue;
        }

        let missing = secrets
            .iter()
            .filter(|secret| !workflow.text.contains(secret.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if missing.is_empty() {
            continue;
        }

        report.error(
            rule_id,
            &workflow.rel_path,
            None,
            format!(
                "deploy workflow does not mention required Wrangler secrets: {}",
                missing.join(", ")
            ),
            "Deploy workflows should pass or document every wrangler.jsonc `secrets.required` name so CI fails early when repository/environment secrets are missing. In GitHub Actions, add entries such as `BETTER_AUTH_SECRET: ${{ secrets.BETTER_AUTH_SECRET }}` or document that `wrangler secret put` supplies the Worker secret before deploy.",
        );
    }
}

fn workflow_mentions_deploy(text: &str) -> bool {
    text.contains("wrangler deploy")
        || text.contains("pnpm deploy")
        || text.contains("bun run deploy")
}

fn check_required_wrangler_envs(
    wrangler: &Value,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let envs = wrangler.get("env").and_then(Value::as_object);
    let missing = REQUIRED_WRANGLER_ENVS
        .iter()
        .filter(|name| envs.is_none_or(|envs| !envs.contains_key(**name)))
        .copied()
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return;
    }

    report.error(
        rule_id,
        wrangler_path,
        None,
        format!(
            "wrangler.jsonc is missing required environments: {}",
            missing.join(", ")
        ),
        "define `env.preview` and `env.production` so deploy targets have explicit Worker bindings and vars",
    );
}

fn check_app_env_values(
    wrangler: &Value,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    check_single_app_env(
        wrangler,
        "top-level",
        "development",
        wrangler_path,
        report,
        rule_id,
    );

    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for (name, env) in envs {
            check_single_app_env(env, name, name, wrangler_path, report, rule_id);
        }
    }
}

fn check_single_app_env(
    config: &Value,
    label: &str,
    expected: &str,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let actual = config
        .get("vars")
        .and_then(|vars| vars.get("APP_ENV"))
        .and_then(Value::as_str);

    if actual == Some(expected) {
        return;
    }

    let found = actual
        .map(|value| format!("found `{value}`"))
        .unwrap_or_else(|| "missing".to_string());
    report.error(
        rule_id,
        wrangler_path,
        None,
        format!("Wrangler `{label}` vars.APP_ENV must be `{expected}` ({found})"),
        "set top-level APP_ENV to `development`; set each `env.<name>` APP_ENV value to that environment name",
    );
}

fn check_hyperdrive_local_connection_strings(
    wrangler: &Value,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    check_config_hyperdrive_local_connection_strings(
        wrangler,
        "top-level",
        wrangler_path,
        report,
        rule_id,
    );

    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for (name, env) in envs {
            check_config_hyperdrive_local_connection_strings(
                env,
                &format!("env.{name}"),
                wrangler_path,
                report,
                rule_id,
            );
        }
    }
}

fn check_config_hyperdrive_local_connection_strings(
    config: &Value,
    label: &str,
    wrangler_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(entries) = config.get("hyperdrive").and_then(Value::as_array) else {
        return;
    };

    for entry in entries {
        if !entry
            .as_object()
            .is_some_and(|object| object.contains_key("localConnectionString"))
        {
            continue;
        }

        let binding = entry
            .get("binding")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        report.error(
            rule_id,
            wrangler_path,
            None,
            format!(
                "Hyperdrive binding `{binding}` in `{label}` must not set localConnectionString"
            ),
            format!(
                "put the local override in .env.example as CLOUDFLARE_HYPERDRIVE_LOCAL_CONNECTION_STRING_{binding}"
            ),
        );
    }
}

fn collect_hyperdrive_local_overrides(wrangler: &Value) -> BTreeSet<String> {
    let mut bindings = BTreeSet::new();
    collect_hyperdrive_bindings(wrangler, &mut bindings);
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            collect_hyperdrive_bindings(env, &mut bindings);
        }
    }
    bindings
        .into_iter()
        .map(|binding| format!("CLOUDFLARE_HYPERDRIVE_LOCAL_CONNECTION_STRING_{binding}"))
        .collect()
}

fn collect_hyperdrive_bindings(config: &Value, bindings: &mut BTreeSet<String>) {
    let Some(entries) = config.get("hyperdrive").and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        if let Some(binding) = entry.get("binding").and_then(Value::as_str) {
            bindings.insert(binding.to_string());
        }
    }
}

fn check_dev_vars_example(
    project: &Project,
    app_root: &str,
    wrangler_runtime_names: &BTreeSet<String>,
    report: &mut Report,
    rule_id: &'static str,
) {
    let rel_path = join(app_root, ".dev.vars.example");
    let Some(keys) = read_env_example(project, &rel_path, report, rule_id) else {
        if !wrangler_runtime_names.is_empty() {
            report.error(
                rule_id,
                rel_path,
                None,
                ".dev.vars.example is missing Wrangler runtime env names",
                format!(
                    "document these Wrangler `vars` and `secrets.required` names in .dev.vars.example: {}",
                    format_set(wrangler_runtime_names)
                ),
            );
        }
        return;
    };

    let missing = difference(wrangler_runtime_names, &keys);
    let extra = difference(&keys, wrangler_runtime_names);
    if missing.is_empty() && extra.is_empty() {
        return;
    }

    let mut details = Vec::new();
    if !missing.is_empty() {
        details.push(format!("missing {}", missing.join(", ")));
    }
    if !extra.is_empty() {
        details.push(format!("extra {}", extra.join(", ")));
    }

    report.error(
        rule_id,
        rel_path,
        None,
        format!(
            ".dev.vars.example does not exactly match Wrangler runtime env names: {}",
            details.join("; ")
        ),
        "keep Worker runtime `vars` and `secrets.required` names in .dev.vars.example only; put Wrangler/build/dev process variables in .env.example",
    );
}

fn check_env_example(
    project: &Project,
    app_root: &str,
    wrangler_runtime_names: &BTreeSet<String>,
    build_dev_vars: &BTreeSet<String>,
    report: &mut Report,
    rule_id: &'static str,
) {
    let rel_path = join(app_root, ".env.example");
    let Some(keys) = read_env_example(project, &rel_path, report, rule_id) else {
        if !build_dev_vars.is_empty() {
            report.error(
                rule_id,
                rel_path,
                None,
                ".env.example is missing build/dev variables",
                format!(
                    "document these non-Wrangler runtime variables in .env.example: {}",
                    format_set(build_dev_vars)
                ),
            );
        }
        return;
    };

    let overlap = intersection(&keys, wrangler_runtime_names);
    if !overlap.is_empty() {
        report.error(
            rule_id,
            &rel_path,
            None,
            format!(
                ".env.example includes Wrangler runtime env names: {}",
                overlap.join(", ")
            ),
            "move Wrangler `vars` and `secrets.required` names to .dev.vars.example so .env.example stays reserved for build and dev tooling",
        );
    }

    let missing = difference(build_dev_vars, &keys);
    if !missing.is_empty() {
        report.error(
            rule_id,
            rel_path,
            None,
            format!(
                ".env.example is missing discovered build/dev variables: {}",
                missing.join(", ")
            ),
            "document import.meta env variables and local Wrangler override variables in .env.example",
        );
    }
}

fn collect_build_dev_vars(
    project: &Project,
    app_root: &str,
    wrangler_runtime_names: &BTreeSet<String>,
    hyperdrive_overrides: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut vars = hyperdrive_overrides.clone();
    for file in &project.files {
        if !is_within_app(&file.rel_path, app_root)
            || file.rel_path.ends_with("wrangler.jsonc")
            || file.rel_path.ends_with(".dev.vars.example")
            || file.rel_path.ends_with(".env.example")
        {
            continue;
        }

        collect_env_accesses(&file.text, "import.meta.env.", &mut vars);
        collect_bracket_env_accesses(&file.text, "import.meta.env[", &mut vars);
    }

    vars.retain(|var| {
        !wrangler_runtime_names.contains(var) && !IMPLICIT_ENV_VARS.contains(&var.as_str())
    });
    vars
}

fn collect_env_accesses(text: &str, marker: &str, vars: &mut BTreeSet<String>) {
    let mut offset = 0;
    while let Some(index) = text[offset..].find(marker) {
        let start = offset + index + marker.len();
        let name = read_identifier(&text[start..]);
        if let Some(name) = name {
            vars.insert(name);
        }
        offset = start;
    }
}

fn collect_bracket_env_accesses(text: &str, marker: &str, vars: &mut BTreeSet<String>) {
    let mut offset = 0;
    while let Some(index) = text[offset..].find(marker) {
        let quote_index = offset + index + marker.len();
        let Some(quote) = text[quote_index..].chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            offset = quote_index;
            continue;
        }
        let value_start = quote_index + quote.len_utf8();
        let Some(value_end) = text[value_start..].find(quote) else {
            break;
        };
        let name = &text[value_start..value_start + value_end];
        if is_env_name(name) {
            vars.insert(name.to_string());
        }
        offset = value_start + value_end;
    }
}

fn read_identifier(text: &str) -> Option<String> {
    let name = text
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();
    if is_env_name(&name) { Some(name) } else { None }
}

fn is_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_uppercase() || first == '_')
        && chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn read_env_example(
    project: &Project,
    rel_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) -> Option<BTreeSet<String>> {
    let path = project.root.join(rel_path);
    if !path.exists() {
        return None;
    }
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            report.error(
                rule_id,
                rel_path,
                None,
                format!("could not read {rel_path}: {err}"),
                "make sure example environment files are readable by repository tools",
            );
            return Some(BTreeSet::new());
        }
    };
    Some(parse_env_keys(&text))
}

fn parse_env_keys(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
            let (key, _) = line.split_once('=')?;
            let key = key.trim();
            if is_env_name(key) {
                Some(key.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn difference(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}

fn intersection(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.intersection(right).cloned().collect()
}

fn format_set(values: &BTreeSet<String>) -> String {
    values.iter().cloned().collect::<Vec<_>>().join(", ")
}

fn is_within_app(rel_path: &str, app_root: &str) -> bool {
    app_root == "." || app_root.is_empty() || rel_path.starts_with(&format!("{}/", app_root))
}

fn join(root: &str, rel_path: &str) -> String {
    if root == "." || root.is_empty() {
        rel_path.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel_path)
    }
}

const ARRAY_BINDING_SECTIONS: &[(&str, &str)] = &[
    ("kv_namespaces", "binding"),
    ("r2_buckets", "binding"),
    ("d1_databases", "binding"),
    ("vectorize", "binding"),
    ("hyperdrive", "binding"),
    ("services", "binding"),
    ("analytics_engine_datasets", "binding"),
    ("dispatch_namespaces", "binding"),
    ("mtls_certificates", "binding"),
    ("send_email", "name"),
    ("pipelines", "binding"),
];

const OBJECT_BINDING_SECTIONS: &[&str] = &[
    "ai",
    "assets",
    "browser",
    "images",
    "version_metadata",
    "worker_loader",
];

const OBJECT_KEY_BINDING_SECTIONS: &[&str] = &["wasm_modules", "text_blobs", "data_blobs"];

const REQUIRED_WRANGLER_ENVS: &[&str] = &["preview", "production"];

const IMPLICIT_ENV_VARS: &[&str] = &[
    "CI", "HOME", "NODE_ENV", "PATH", "PWD", "SHELL", "TERM", "TMPDIR", "USER",
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::{collect_surface, collect_worker_env_names_from_config, expected_surface};

    #[test]
    fn send_email_uses_name_as_its_binding_field() {
        let config = json!({
            "send_email": [{ "name": "MAILER", "destination_address": "ops@example.com" }]
        });

        assert!(collect_surface(&config).contains("send_email.MAILER"));
        assert!(collect_worker_env_names_from_config(&config).contains("MAILER"));
    }

    #[test]
    fn production_surface_omits_dev_prefixed_vars_only() {
        let baseline = [
            "vars.APP_ENV".to_string(),
            "vars.DEV_SEED_DATA".to_string(),
            "secrets.BETTER_AUTH_SECRET".to_string(),
            "hyperdrive.DB".to_string(),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        let production = expected_surface(&baseline, "env.production");
        assert!(!production.contains("vars.DEV_SEED_DATA"));
        assert!(production.contains("vars.APP_ENV"));
        assert!(production.contains("secrets.BETTER_AUTH_SECRET"));
        assert!(production.contains("hyperdrive.DB"));

        assert_eq!(expected_surface(&baseline, "env.preview"), baseline);
    }
}
