use std::collections::BTreeSet;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::api_layer::collect_route_contracts;
use crate::rules::common::{join, named_object_blocks, object_properties};
use crate::rules::wrangler_environment::collect_worker_env_names;
use crate::rules::{Report, Rule};
use crate::structured::parse_jsonc;

pub struct GeneratedContractSyncRule;

impl Rule for GeneratedContractSyncRule {
    fn id(&self) -> &'static str {
        "generated-contract-sync"
    }

    fn category(&self) -> &'static str {
        "contracts"
    }

    fn description(&self) -> &'static str {
        "detects stale generated OpenAPI and Cloudflare binding contracts"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.web_apps {
            check_openapi_sync(project, app_root, report, self.id());
        }

        let roots = config
            .web_apps
            .iter()
            .chain(&config.worker_apps)
            .collect::<BTreeSet<_>>();
        for app_root in roots {
            check_worker_sync(project, app_root, report, self.id());
        }
    }
}

fn check_openapi_sync(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let generated_path = join(app_root, "src/lib/api/v1.d.ts");
    let Some(generated) = project.read(&generated_path) else {
        return;
    };
    let generated_pairs = openapi_method_path_pairs(generated);
    let mut checked = BTreeSet::new();
    for route in collect_route_contracts(project, app_root) {
        let (Some(method), Some(path)) = (route.method, route.path) else {
            continue;
        };
        let pair = (method.to_ascii_lowercase(), path);
        if !checked.insert(pair.clone()) || generated_pairs.contains(&pair) {
            continue;
        }
        report.error(
            rule_id,
            &generated_path,
            None,
            format!(
                "generated API contract is missing `{} {}` from {}:{}",
                pair.0.to_ascii_uppercase(),
                pair.1,
                route.file,
                route.line
            ),
            "Regenerate src/lib/api/v1.d.ts with `pnpm gen:api`. Generated contract sync findings are attached to the generated file and cannot be hidden by source suppressions.",
        );
    }
}

fn openapi_method_path_pairs(text: &str) -> BTreeSet<(String, String)> {
    let mut pairs = BTreeSet::new();
    let mut blocks = named_object_blocks(text, "interface", "paths");
    blocks.extend(named_object_blocks(text, "type", "paths"));
    for block in blocks {
        let Some(paths) = object_properties(block) else {
            continue;
        };
        for path in paths
            .into_iter()
            .filter(|property| property.name.starts_with('/'))
        {
            let Some(methods) = object_properties(path.value) else {
                continue;
            };
            for method in methods {
                let method_name = method.name.to_ascii_lowercase();
                if HTTP_METHODS.contains(&method_name.as_str()) {
                    pairs.insert((method_name, path.name.clone()));
                }
            }
        }
    }
    pairs
}

fn check_worker_sync(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let wrangler_path = join(app_root, "wrangler.jsonc");
    let generated_path = join(app_root, "worker-configuration.d.ts");
    let (Some(wrangler), Some(generated)) =
        (project.read(&wrangler_path), project.read(&generated_path))
    else {
        return;
    };
    let Ok(wrangler) = parse_jsonc(wrangler) else {
        return;
    };
    let expected = collect_worker_env_names(&wrangler);
    let actual = worker_env_names(generated);
    for missing in expected.difference(&actual) {
        report.error(
            rule_id,
            &generated_path,
            None,
            format!("generated Worker contract is missing binding `{missing}`"),
            "Regenerate worker-configuration.d.ts with `pnpm gen:cf`. Generated contract sync findings are attached to the generated file and cannot be suppressed there.",
        );
    }
}

fn worker_env_names(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut blocks = named_object_blocks(text, "interface", "Env");
    blocks.extend(named_object_blocks(text, "type", "Env"));
    blocks.extend(named_object_blocks(text, "interface", "__BaseEnv_Env"));
    for block in blocks {
        let Some(properties) = object_properties(block) else {
            continue;
        };
        names.extend(properties.into_iter().map(|property| property.name));
    }
    names
}

const HTTP_METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

#[cfg(test)]
mod tests {
    use super::{openapi_method_path_pairs, worker_env_names};

    #[test]
    fn parses_method_path_pairs_from_generated_paths_interface() {
        let pairs = openapi_method_path_pairs(
            r#"
export interface paths {
  "/api/v1/users": {
    get: operations["listUsers"];
    post: operations["createUser"];
  };
}
"#,
        );
        assert!(pairs.contains(&("get".to_string(), "/api/v1/users".to_string())));
        assert!(pairs.contains(&("post".to_string(), "/api/v1/users".to_string())));
    }

    #[test]
    fn parses_worker_bindings_from_namespaced_env_interface() {
        let names = worker_env_names(
            "declare namespace Cloudflare { interface Env { DB: D1Database; ASSETS: Fetcher; } }",
        );
        assert_eq!(names, ["ASSETS".to_string(), "DB".to_string()].into());
    }

    #[test]
    fn parses_current_wrangler_base_env_interface() {
        let names = worker_env_names(
            "interface __BaseEnv_Env { DB: Hyperdrive; APP_ENV: string; }\ninterface Env extends __BaseEnv_Env {}",
        );
        assert_eq!(names, ["APP_ENV".to_string(), "DB".to_string()].into());
    }
}
