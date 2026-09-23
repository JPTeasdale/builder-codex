use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::join;
use crate::rules::{Report, Rule};
use crate::structured::parse_jsonc;

pub struct WranglerResourceIsolationRule;

impl Rule for WranglerResourceIsolationRule {
    fn id(&self) -> &'static str {
        "wrangler-resource-isolation"
    }

    fn category(&self) -> &'static str {
        "cloudflare"
    }

    fn description(&self) -> &'static str {
        "keeps preview and production Cloudflare resources isolated"
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
            let Some(envs) = wrangler.get("env").and_then(Value::as_object) else {
                continue;
            };
            let (Some(preview), Some(production)) = (envs.get("preview"), envs.get("production"))
            else {
                continue;
            };

            let preview_resources = collect_resources(&wrangler, preview, "preview");
            let production_resources = collect_resources(&wrangler, production, "production");

            check_preview_placeholders(&wrangler_path, text, &preview_resources, report, self.id());
            check_shared_resources(
                &wrangler_path,
                text,
                &preview_resources,
                &production_resources,
                report,
                self.id(),
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Resource {
    kind: &'static str,
    binding: String,
    identifier: String,
    binding_field: &'static str,
}

fn collect_resources(wrangler: &Value, config: &Value, environment: &str) -> Vec<Resource> {
    let mut resources = Vec::new();

    for spec in RESOURCE_SPECS {
        collect_array_resources(config, spec, &mut resources);
    }
    collect_queue_consumers(config, &mut resources);
    collect_durable_objects(wrangler, config, environment, &mut resources);

    resources.sort_by(|left, right| {
        left.kind
            .cmp(right.kind)
            .then_with(|| left.binding.cmp(&right.binding))
            .then_with(|| left.identifier.cmp(&right.identifier))
    });
    resources
}

fn collect_array_resources(config: &Value, spec: &ResourceSpec, resources: &mut Vec<Resource>) {
    let Some(entries) = nested_value(config, spec.path).and_then(Value::as_array) else {
        return;
    };
    for entry in entries {
        let Some(binding) = entry.get(spec.binding_field).and_then(Value::as_str) else {
            continue;
        };
        let Some(identifier) = composite_identifier(entry, spec.identifier_fields) else {
            continue;
        };
        resources.push(Resource {
            kind: spec.kind,
            binding: binding.to_string(),
            identifier,
            binding_field: spec.binding_field,
        });
    }
}

fn collect_queue_consumers(config: &Value, resources: &mut Vec<Resource>) {
    let Some(consumers) = nested_value(config, &["queues", "consumers"]).and_then(Value::as_array)
    else {
        return;
    };
    for (index, consumer) in consumers.iter().enumerate() {
        let Some(queue) = consumer.get("queue").and_then(Value::as_str) else {
            continue;
        };
        resources.push(Resource {
            kind: "Queue consumer",
            binding: format!("consumer[{index}]"),
            identifier: queue.to_string(),
            binding_field: "queue",
        });
    }
}

fn collect_durable_objects(
    wrangler: &Value,
    config: &Value,
    environment: &str,
    resources: &mut Vec<Resource>,
) {
    let Some(bindings) =
        nested_value(config, &["durable_objects", "bindings"]).and_then(Value::as_array)
    else {
        return;
    };
    for binding in bindings {
        let Some(name) = binding.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(class_name) = binding.get("class_name").and_then(Value::as_str) else {
            continue;
        };
        let identifier =
            if let Some(script_name) = binding.get("script_name").and_then(Value::as_str) {
                format!(
                    "external:{script_name}:{}:{class_name}",
                    binding
                        .get("environment")
                        .and_then(Value::as_str)
                        .unwrap_or("production")
                )
            } else {
                format!(
                    "local:{}:{class_name}",
                    effective_worker_name(wrangler, config, environment)
                )
            };
        resources.push(Resource {
            kind: "Durable Object",
            binding: name.to_string(),
            identifier,
            binding_field: "name",
        });
    }
}

fn effective_worker_name(wrangler: &Value, config: &Value, environment: &str) -> String {
    if let Some(name) = config.get("name").and_then(Value::as_str) {
        return name.to_string();
    }
    if let Some(name) = wrangler.get("name").and_then(Value::as_str) {
        return format!("{name}-{environment}");
    }
    format!("<current-worker-{environment}>")
}

fn composite_identifier(entry: &Value, fields: &[&str]) -> Option<String> {
    let values = fields
        .iter()
        .map(|field| entry.get(field).and_then(Value::as_str).unwrap_or(""))
        .collect::<Vec<_>>();
    values
        .iter()
        .any(|value| !value.is_empty())
        .then(|| values.join(":"))
}

fn check_shared_resources(
    wrangler_path: &str,
    text: &str,
    preview: &[Resource],
    production: &[Resource],
    report: &mut Report,
    rule_id: &'static str,
) {
    let production_by_binding = production
        .iter()
        .map(|resource| ((resource.kind, resource.binding.as_str()), resource))
        .collect::<BTreeMap<_, _>>();
    let production_queue_consumers = production
        .iter()
        .filter(|resource| resource.kind == "Queue consumer")
        .map(|resource| resource.identifier.as_str())
        .collect::<BTreeSet<_>>();

    for resource in preview {
        let shared = if resource.kind == "Queue consumer" {
            production_queue_consumers.contains(resource.identifier.as_str())
        } else {
            production_by_binding
                .get(&(resource.kind, resource.binding.as_str()))
                .is_some_and(|production| production.identifier == resource.identifier)
        };
        if !shared || resource.identifier.is_empty() {
            continue;
        }

        report.error(
            rule_id,
            wrangler_path,
            resource_line(text, "preview", resource),
            format!(
                "preview and production share mutable {} resource `{}` for binding `{}`",
                resource.kind, resource.identifier, resource.binding
            ),
            "provision separate preview and production resources. A deliberate exception must be local and reason-bearing: `// ultralint: allow wrangler-resource-isolation -- <specific reason> until=YYYY-MM-DD` immediately before the preview binding",
        );
    }
}

fn check_preview_placeholders(
    wrangler_path: &str,
    text: &str,
    preview: &[Resource],
    report: &mut Report,
    rule_id: &'static str,
) {
    for resource in preview {
        let components = resource
            .identifier
            .split(':')
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if !components.iter().any(|value| is_deploy_placeholder(value)) {
            continue;
        }
        report.error(
            rule_id,
            wrangler_path,
            resource_line(text, "preview", resource),
            format!(
                "preview {} binding `{}` contains deployable placeholder identifier `{}`",
                resource.kind, resource.binding, resource.identifier
            ),
            "replace the placeholder with a provisioned preview resource identifier before running `pnpm exec wrangler deploy --env preview`",
        );
    }
}

fn is_deploy_placeholder(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return true;
    }
    let compact = value.replace(['-', '_', ' '], "");
    value.contains("__placeholder__")
        || value.contains("replace-me")
        || value.contains("replace_me")
        || value.contains("<placeholder")
        || value.contains("<your-")
        || compact == "placeholder"
        || compact == "changeme"
        || compact == "previewid"
        || compact == "previewdb"
        || compact == "previewbucket"
        || compact == "previewqueue"
        || compact == "previewservice"
        || (!compact.is_empty() && compact.chars().all(|ch| ch == '0'))
}

fn resource_line(text: &str, environment: &str, resource: &Resource) -> Option<usize> {
    let lines = text.lines().collect::<Vec<_>>();
    let env_marker = format!("\"{environment}\"");
    let start = lines.iter().position(|line| line.contains(&env_marker))?;
    let binding_marker = format!("\"{}\"", resource.binding);
    let field_marker = format!("\"{}\"", resource.binding_field);
    let binding_line = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.contains(&field_marker) && line.contains(&binding_marker))
        .map(|(index, _)| index)?;
    let entry_line = (start + 1..=binding_line)
        .rev()
        .find(|index| lines[*index].contains('{'))
        .unwrap_or(binding_line);
    Some(entry_line + 1)
}

fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

struct ResourceSpec {
    kind: &'static str,
    path: &'static [&'static str],
    binding_field: &'static str,
    identifier_fields: &'static [&'static str],
}

const RESOURCE_SPECS: &[ResourceSpec] = &[
    ResourceSpec {
        kind: "Hyperdrive",
        path: &["hyperdrive"],
        binding_field: "binding",
        identifier_fields: &["id"],
    },
    ResourceSpec {
        kind: "R2",
        path: &["r2_buckets"],
        binding_field: "binding",
        identifier_fields: &["bucket_name"],
    },
    ResourceSpec {
        kind: "KV",
        path: &["kv_namespaces"],
        binding_field: "binding",
        identifier_fields: &["id"],
    },
    ResourceSpec {
        kind: "D1",
        path: &["d1_databases"],
        binding_field: "binding",
        identifier_fields: &["database_id"],
    },
    ResourceSpec {
        kind: "Queue producer",
        path: &["queues", "producers"],
        binding_field: "binding",
        identifier_fields: &["queue"],
    },
    ResourceSpec {
        kind: "service",
        path: &["services"],
        binding_field: "binding",
        identifier_fields: &["service", "environment", "entrypoint"],
    },
];

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{collect_resources, is_deploy_placeholder};

    #[test]
    fn mutable_resources_are_collected_by_binding_and_remote_identifier() {
        let wrangler = json!({ "name": "example" });
        let config = json!({
            "r2_buckets": [{ "binding": "UPLOADS", "bucket_name": "example-preview" }],
            "services": [{ "binding": "AUTH", "service": "auth", "environment": "preview" }]
        });
        let resources = collect_resources(&wrangler, &config, "preview");

        assert!(resources.iter().any(|resource| {
            resource.binding == "UPLOADS" && resource.identifier == "example-preview"
        }));
        assert!(resources.iter().any(|resource| {
            resource.binding == "AUTH" && resource.identifier == "auth:preview:"
        }));
    }

    #[test]
    fn local_durable_objects_use_the_effective_worker_environment() {
        let wrangler = json!({ "name": "example" });
        let binding = json!({
            "durable_objects": {
                "bindings": [{ "name": "ROOM", "class_name": "Room" }]
            }
        });

        let preview = collect_resources(&wrangler, &binding, "preview");
        let production = collect_resources(&wrangler, &binding, "production");
        assert_ne!(preview[0].identifier, production[0].identifier);
    }

    #[test]
    fn preview_names_are_valid_but_generic_placeholders_are_not() {
        assert!(!is_deploy_placeholder("acme-images-preview"));
        assert!(is_deploy_placeholder("preview-bucket"));
        assert!(is_deploy_placeholder(
            "00000000-0000-0000-0000-000000000000"
        ));
    }
}
