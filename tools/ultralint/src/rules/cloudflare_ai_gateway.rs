use std::collections::{BTreeSet, VecDeque};

use serde_json::Value;

use crate::analysis::{CallFact, TsAnalysis};
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{is_test_file, is_ts_source_file, join};
use crate::rules::{Report, Rule};
use crate::structured::parse_jsonc;

pub struct CloudflareAiGatewayBindingRule;

impl Rule for CloudflareAiGatewayBindingRule {
    fn id(&self) -> &'static str {
        "cloudflare-ai-gateway-binding"
    }

    fn category(&self) -> &'static str {
        "cloudflare"
    }

    fn description(&self) -> &'static str {
        "requires Cloudflare AI Gateway calls to go through the Worker AI binding"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.worker_apps {
            let wrangler_path = join(app_root, "wrangler.jsonc");
            let Some(text) = project.read(&wrangler_path) else {
                continue;
            };
            let Ok(wrangler) = parse_jsonc(text) else {
                continue;
            };
            if !has_ai_binding(&wrangler) {
                continue;
            }

            for file in runtime_typescript_files(project, app_root, &wrangler) {
                let Some(analysis) = &file.ts else {
                    continue;
                };
                for violation in ai_call_violations(analysis) {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(violation.line),
                        violation.message,
                        violation.help,
                    );
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct AiViolation {
    line: usize,
    message: &'static str,
    help: &'static str,
}

fn has_ai_binding(wrangler: &Value) -> bool {
    config_has_ai_binding(wrangler)
        || wrangler
            .get("env")
            .and_then(Value::as_object)
            .is_some_and(|envs| envs.values().any(config_has_ai_binding))
}

fn config_has_ai_binding(config: &Value) -> bool {
    config
        .get("ai")
        .and_then(|ai| ai.get("binding"))
        .and_then(Value::as_str)
        .is_some_and(|binding| !binding.trim().is_empty())
}

fn runtime_typescript_files<'a>(
    project: &'a Project,
    app_root: &str,
    wrangler: &Value,
) -> Vec<&'a ProjectFile> {
    let mut pending = collect_entrypoints(project, app_root, wrangler)
        .into_iter()
        .collect::<VecDeque<_>>();
    let mut visited = BTreeSet::new();
    let mut files = Vec::new();

    while let Some(path) = pending.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let Some(file) = project.file(&path) else {
            continue;
        };
        if file.generated || is_test_file(&file.rel_path) || !is_ts_source_file(&file.rel_path) {
            continue;
        }
        files.push(file);

        let Some(analysis) = &file.ts else {
            continue;
        };
        for import in &analysis.imports {
            if import.type_only {
                continue;
            }
            let Some(resolved) = project.resolve_import(&file.rel_path, &import.source) else {
                continue;
            };
            if is_within_app(&resolved, app_root) && is_ts_source_file(&resolved) {
                pending.push_back(resolved);
            }
        }
    }

    files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    files
}

fn collect_entrypoints(project: &Project, app_root: &str, wrangler: &Value) -> BTreeSet<String> {
    let mut entries = BTreeSet::new();
    collect_config_entrypoint(project, app_root, wrangler, &mut entries);
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            collect_config_entrypoint(project, app_root, env, &mut entries);
        }
    }
    entries
}

fn collect_config_entrypoint(
    project: &Project,
    app_root: &str,
    config: &Value,
    entries: &mut BTreeSet<String>,
) {
    let Some(main) = config.get("main").and_then(Value::as_str) else {
        return;
    };
    let main = main.trim_start_matches("./");
    let candidate = join(app_root, main);
    for path in [
        candidate.clone(),
        format!("{candidate}.ts"),
        format!("{candidate}.tsx"),
        format!("{candidate}/index.ts"),
        format!("{candidate}/index.tsx"),
    ] {
        if project.is_file(&path) {
            entries.insert(path);
            return;
        }
    }
}

fn ai_call_violations(analysis: &TsAnalysis) -> Vec<AiViolation> {
    let mut violations = Vec::new();
    for call in &analysis.calls {
        let endpoint = direct_ai_endpoint(&call.text);
        let actual_ai_call = endpoint.is_some() || is_ai_call_context(call);

        if let Some(endpoint) = endpoint.filter(|_| is_network_call(&call.callee)) {
            let (message, help) = match endpoint {
                DirectAiEndpoint::Rest => (
                    "direct Cloudflare AI REST URL is used in a runtime AI call",
                    "use the Worker AI binding: env.AI.run(model, input, { gateway: { id } })",
                ),
                DirectAiEndpoint::Gateway => (
                    "direct AI Gateway provider URL is used in a runtime AI call",
                    "use env.AI.gateway(id).getUrl(provider) for SDK integrations, or env.AI.run with gateway options",
                ),
            };
            violations.push(AiViolation {
                line: call.line,
                message,
                help,
            });
        }

        if actual_ai_call && contains_cloudflare_credentials(&call.text) {
            violations.push(AiViolation {
                line: call.line,
                message: "Cloudflare API token/account env is used in a runtime AI call",
                help: "use the wrangler.jsonc AI binding and env.AI; runtime model calls must not depend on Cloudflare REST API credentials",
            });
        }
    }
    violations
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectAiEndpoint {
    Rest,
    Gateway,
}

fn direct_ai_endpoint(text: &str) -> Option<DirectAiEndpoint> {
    let compact = text.replace(char::is_whitespace, "").to_ascii_lowercase();
    if compact.contains("gateway.ai.cloudflare.com/v1") {
        return Some(DirectAiEndpoint::Gateway);
    }
    if compact.contains("api.cloudflare.com/client/v4/accounts")
        && (compact.contains("/ai/run/")
            || compact.contains("/ai-gateway/")
            || compact.contains("/ai/v1/"))
    {
        return Some(DirectAiEndpoint::Rest);
    }
    None
}

fn is_network_call(callee: &str) -> bool {
    let callee = callee.to_ascii_lowercase();
    callee == "fetch"
        || callee.ends_with(".fetch")
        || callee.ends_with(".request")
        || callee.ends_with(".get")
        || callee.ends_with(".post")
        || callee.ends_with(".put")
        || callee.ends_with(".create")
}

fn is_ai_call_context(call: &CallFact) -> bool {
    let callee = call.callee.to_ascii_lowercase();
    let text = call.text.to_ascii_lowercase();
    callee.ends_with(".ai.run")
        || callee.ends_with(".messages.create")
        || callee.ends_with(".responses.create")
        || callee.ends_with(".completions.create")
        || callee.ends_with(".generatecontent")
        || text.contains("/ai/run/")
        || text.contains("ai-gateway")
        || text.contains("gateway.ai.cloudflare.com")
}

fn contains_cloudflare_credentials(text: &str) -> bool {
    text.contains("CLOUDFLARE_API_TOKEN") || text.contains("CLOUDFLARE_ACCOUNT_ID")
}

fn is_within_app(rel_path: &str, app_root: &str) -> bool {
    app_root == "."
        || app_root.is_empty()
        || rel_path.starts_with(&format!("{}/", app_root.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::analysis::analyze_typescript;

    use super::{ai_call_violations, has_ai_binding};

    #[test]
    fn activation_requires_an_ai_binding_in_some_environment() {
        assert!(!has_ai_binding(&json!({ "r2_buckets": [] })));
        assert!(has_ai_binding(&json!({
            "env": { "preview": { "ai": { "binding": "AI" } } }
        })));
    }

    #[test]
    fn credentials_are_only_reported_in_ai_call_context() {
        let analysis = analyze_typescript(
            "src/worker.ts",
            r#"
                fetch("https://api.cloudflare.com/client/v4/accounts/account/r2/buckets", {
                    headers: { Authorization: env.CLOUDFLARE_API_TOKEN },
                });
                fetch("https://api.cloudflare.com/client/v4/accounts/account/ai/run/model", {
                    headers: { Authorization: env.CLOUDFLARE_API_TOKEN },
                });
            "#,
        );

        let violations = ai_call_violations(&analysis);
        assert_eq!(violations.len(), 2);
        assert!(violations.iter().all(|violation| violation.line > 4));
    }

    #[test]
    fn strings_outside_network_calls_are_not_ai_violations() {
        let analysis = analyze_typescript(
            "src/worker.ts",
            r#"console.info("gateway.ai.cloudflare.com/v1 is documented elsewhere");"#,
        );
        assert!(ai_call_violations(&analysis).is_empty());
    }
}
