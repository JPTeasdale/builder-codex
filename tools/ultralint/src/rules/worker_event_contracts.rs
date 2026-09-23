use std::collections::BTreeSet;

use serde_json::Value;

use crate::analysis::ImportKind;
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{is_ts_source_file, join};
use crate::rules::{Report, Rule};
use crate::structured::parse_jsonc;

pub struct WorkerEventContractsRule;

impl Rule for WorkerEventContractsRule {
    fn id(&self) -> &'static str {
        "worker-event-contracts"
    }

    fn category(&self) -> &'static str {
        "cloudflare"
    }

    fn description(&self) -> &'static str {
        "validates Worker queue, scheduled, and Durable Object event contracts"
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

            let requirements = collect_requirements(&wrangler);
            if requirements.is_empty() {
                continue;
            }

            for consumer in &requirements.queue_consumers {
                if consumer
                    .dead_letter_queue
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                {
                    continue;
                }
                report.error(
                    self.id(),
                    &wrangler_path,
                    find_value_line(text, "queue", &consumer.queue),
                    format!(
                        "queue consumer `{}` has no explicit dead_letter_queue policy",
                        consumer.queue
                    ),
                    "provision a dead-letter queue with `pnpm exec wrangler queues create <queue>-dlq`, then set `dead_letter_queue` on this consumer",
                );
            }

            let entrypoints = collect_entrypoint_files(project, app_root, &wrangler);
            if !requirements.queue_consumers.is_empty() {
                check_handler(
                    project,
                    &entrypoints,
                    &wrangler_path,
                    "queue",
                    report,
                    self.id(),
                );
            }
            if requirements.scheduled {
                check_handler(
                    project,
                    &entrypoints,
                    &wrangler_path,
                    "scheduled",
                    report,
                    self.id(),
                );
            }

            let migration_history = collect_migration_history(&wrangler);
            for class_name in &requirements.local_durable_object_classes {
                check_durable_object_export(
                    project,
                    &entrypoints,
                    &wrangler_path,
                    class_name,
                    report,
                    self.id(),
                );
                if !migration_history.active.contains(class_name) {
                    let detail = if migration_history.orphaned_renames.contains(class_name) {
                        "rename history does not trace back to a created class"
                    } else {
                        "class is absent from new_classes/new_sqlite_classes and valid renamed_classes history"
                    };
                    report.error(
                        self.id(),
                        &wrangler_path,
                        find_value_line(text, "class_name", class_name),
                        format!(
                            "local Durable Object class `{class_name}` has incomplete migration history: {detail}"
                        ),
                        "add an append-only Wrangler migration that creates or validly renames the class before `pnpm exec wrangler deploy --env preview`; never rewrite already-deployed migration tags",
                    );
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct EventRequirements {
    queue_consumers: Vec<QueueConsumer>,
    scheduled: bool,
    local_durable_object_classes: BTreeSet<String>,
}

impl EventRequirements {
    fn is_empty(&self) -> bool {
        self.queue_consumers.is_empty()
            && !self.scheduled
            && self.local_durable_object_classes.is_empty()
    }
}

#[derive(Debug)]
struct QueueConsumer {
    queue: String,
    dead_letter_queue: Option<String>,
}

fn collect_requirements(wrangler: &Value) -> EventRequirements {
    let mut requirements = EventRequirements::default();
    collect_config_requirements(wrangler, &mut requirements);
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            collect_config_requirements(env, &mut requirements);
        }
    }
    requirements
}

fn collect_config_requirements(config: &Value, requirements: &mut EventRequirements) {
    if let Some(consumers) =
        nested_value(config, &["queues", "consumers"]).and_then(Value::as_array)
    {
        for consumer in consumers {
            let Some(queue) = consumer.get("queue").and_then(Value::as_str) else {
                continue;
            };
            requirements.queue_consumers.push(QueueConsumer {
                queue: queue.to_string(),
                dead_letter_queue: consumer
                    .get("dead_letter_queue")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }

    requirements.scheduled |= config
        .get("triggers")
        .and_then(|triggers| triggers.get("crons"))
        .and_then(Value::as_array)
        .is_some_and(|crons| !crons.is_empty());

    if let Some(bindings) =
        nested_value(config, &["durable_objects", "bindings"]).and_then(Value::as_array)
    {
        for binding in bindings {
            if binding.get("script_name").and_then(Value::as_str).is_some() {
                continue;
            }
            if let Some(class_name) = binding.get("class_name").and_then(Value::as_str) {
                requirements
                    .local_durable_object_classes
                    .insert(class_name.to_string());
            }
        }
    }
}

fn collect_entrypoint_files<'a>(
    project: &'a Project,
    app_root: &str,
    wrangler: &Value,
) -> Vec<&'a ProjectFile> {
    let mut paths = BTreeSet::new();
    collect_config_main(project, app_root, wrangler, &mut paths);
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            collect_config_main(project, app_root, env, &mut paths);
        }
    }
    paths.iter().filter_map(|path| project.file(path)).collect()
}

fn collect_config_main(
    project: &Project,
    app_root: &str,
    config: &Value,
    paths: &mut BTreeSet<String>,
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
        if project.is_file(&path) && is_ts_source_file(&path) {
            paths.insert(path);
            return;
        }
    }
}

fn check_handler(
    _project: &Project,
    entrypoints: &[&ProjectFile],
    wrangler_path: &str,
    handler: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let statuses = entrypoints
        .iter()
        .map(|file| (*file, exported_handler_status(&file.text, handler)))
        .collect::<Vec<_>>();
    if statuses
        .iter()
        .any(|(_, status)| *status == Resolution::Present)
    {
        return;
    }
    if let Some((file, _)) = statuses
        .iter()
        .find(|(_, status)| *status == Resolution::Ambiguous)
    {
        report.warning(
            rule_id,
            &file.rel_path,
            find_token_line(&file.text, "export default"),
            format!(
                "could not prove that the re-exported or factory-created Worker exposes `{handler}`"
            ),
            format!(
                "make the `{handler}` handler visible in the default Worker export, or verify the re-export/factory resolves to an ExportedHandler with `{handler}`"
            ),
        );
        return;
    }

    let (path, line) = entrypoints
        .first()
        .map(|file| {
            (
                file.rel_path.as_str(),
                find_token_line(&file.text, "export default"),
            )
        })
        .unwrap_or((wrangler_path, None));
    report.error(
        rule_id,
        path,
        line,
        format!("Wrangler config requires an exported `{handler}` Worker handler"),
        format!("add `{handler}` to the default Worker export so the configured event has a runtime handler"),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolution {
    Present,
    Ambiguous,
    Missing,
}

fn exported_handler_status(source: &str, handler: &str) -> Resolution {
    let sanitized = sanitize_typescript(source);
    if let Some(index) = sanitized.find("export default") {
        let start = index + "export default".len();
        let rest = sanitized[start..].trim_start();
        if let Some(body) = rest.strip_prefix('{') {
            return if block_has_member(body, handler) {
                Resolution::Present
            } else {
                Resolution::Missing
            };
        }
        if let Some(class) = rest.strip_prefix("class") {
            let Some(body_start) = class.find('{') else {
                return Resolution::Ambiguous;
            };
            return if block_has_member(&class[body_start + 1..], handler) {
                Resolution::Present
            } else {
                Resolution::Missing
            };
        }
        let expression = rest.split([';', '\n']).next().unwrap_or_default().trim();
        if let Some(identifier) = read_identifier(expression)
            && let Some(object_body) = local_object_body(&sanitized, identifier)
        {
            return if block_has_member(object_body, handler) {
                Resolution::Present
            } else {
                Resolution::Missing
            };
        }
        return Resolution::Ambiguous;
    }
    if sanitized.contains("export {") && sanitized.contains(" from ")
        || sanitized.contains("export * from")
    {
        Resolution::Ambiguous
    } else {
        Resolution::Missing
    }
}

fn local_object_body<'a>(source: &'a str, identifier: &str) -> Option<&'a str> {
    for declaration in [
        format!("const {identifier}"),
        format!("let {identifier}"),
        format!("var {identifier}"),
    ] {
        let Some(start) = source
            .find(&declaration)
            .map(|index| index + declaration.len())
        else {
            continue;
        };
        let Some(equals) = source[start..].find('=').map(|index| index + start) else {
            continue;
        };
        let body = source[equals + 1..].trim_start();
        if let Some(body) = body.strip_prefix('{') {
            return Some(body);
        }
    }
    None
}

fn block_has_member(body: &str, member: &str) -> bool {
    let bytes = body.as_bytes();
    let mut depth = 1usize;
    let mut index = 0usize;
    while index < bytes.len() && depth > 0 {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if depth == 1 && starts_identifier(body, index, member) {
            let rest = body[index + member.len()..].trim_start();
            if rest.starts_with('(')
                || rest.starts_with(':')
                || rest.starts_with(',')
                || rest.starts_with('}')
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn check_durable_object_export(
    project: &Project,
    entrypoints: &[&ProjectFile],
    wrangler_path: &str,
    class_name: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let mut ambiguous_file = None;
    for file in entrypoints {
        match durable_object_export_status(project, file, class_name) {
            Resolution::Present => return,
            Resolution::Ambiguous => ambiguous_file = Some(*file),
            Resolution::Missing => {}
        }
    }
    if let Some(file) = ambiguous_file {
        report.warning(
            rule_id,
            &file.rel_path,
            find_token_line(&file.text, class_name),
            format!(
                "could not prove that re-export/factory resolution exports Durable Object class `{class_name}`"
            ),
            "prefer a direct named class export from the Worker entrypoint, or verify the re-export/factory resolves to the configured class",
        );
    } else {
        report.error(
            rule_id,
            entrypoints
                .first()
                .map(|file| file.rel_path.as_str())
                .unwrap_or(wrangler_path),
            None,
            format!("local Durable Object class `{class_name}` is not exported by the Worker"),
            format!("export the class as `export class {class_name} extends DurableObject` from the Worker entrypoint"),
        );
    }
}

fn durable_object_export_status(
    project: &Project,
    entrypoint: &ProjectFile,
    class_name: &str,
) -> Resolution {
    if directly_exports_class(&entrypoint.text, class_name) {
        return Resolution::Present;
    }

    let sanitized = sanitize_typescript(&entrypoint.text);
    let export_marker = format!("export {{ {class_name}");
    let compact_export_marker = format!("export {{{class_name}");
    if sanitized.contains(&export_marker) || sanitized.contains(&compact_export_marker) {
        let Some(analysis) = &entrypoint.ts else {
            return Resolution::Ambiguous;
        };
        for import in &analysis.imports {
            if import.kind != ImportKind::ReExport {
                continue;
            }
            let Some(target) = project.resolve_import(&entrypoint.rel_path, &import.source) else {
                continue;
            };
            if project
                .read(&target)
                .is_some_and(|source| directly_exports_class(source, class_name))
            {
                return Resolution::Present;
            }
        }
        return Resolution::Ambiguous;
    }

    if sanitized.contains("export * from")
        || sanitized.contains(&format!("export const {class_name}"))
        || sanitized.contains(&format!("export {{ {class_name} }}"))
    {
        Resolution::Ambiguous
    } else {
        Resolution::Missing
    }
}

fn directly_exports_class(source: &str, class_name: &str) -> bool {
    let source = sanitize_typescript(source);
    source.contains(&format!("export class {class_name}"))
        || source.contains(&format!("export default class {class_name}"))
}

#[derive(Debug, Default)]
struct MigrationHistory {
    active: BTreeSet<String>,
    orphaned_renames: BTreeSet<String>,
}

fn collect_migration_history(wrangler: &Value) -> MigrationHistory {
    let mut history = MigrationHistory::default();
    let mut migrations = Vec::new();
    if let Some(entries) = wrangler.get("migrations").and_then(Value::as_array) {
        migrations.extend(entries);
    }
    if let Some(envs) = wrangler.get("env").and_then(Value::as_object) {
        for env in envs.values() {
            if let Some(entries) = env.get("migrations").and_then(Value::as_array) {
                migrations.extend(entries);
            }
        }
    }

    for migration in migrations {
        for field in ["new_classes", "new_sqlite_classes"] {
            if let Some(classes) = migration.get(field).and_then(Value::as_array) {
                history
                    .active
                    .extend(classes.iter().filter_map(Value::as_str).map(str::to_string));
            }
        }
        if let Some(renames) = migration.get("renamed_classes").and_then(Value::as_array) {
            for rename in renames {
                let (Some(from), Some(to)) = (
                    rename.get("from").and_then(Value::as_str),
                    rename.get("to").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if history.active.remove(from) {
                    history.active.insert(to.to_string());
                } else {
                    history.orphaned_renames.insert(to.to_string());
                }
            }
        }
        if let Some(deleted) = migration.get("deleted_classes").and_then(Value::as_array) {
            for class_name in deleted.iter().filter_map(Value::as_str) {
                history.active.remove(class_name);
            }
        }
    }
    history
}

fn sanitize_typescript(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while let Some(ch) = chars.next() {
        if line_comment {
            if ch == '\n' {
                line_comment = false;
                output.push('\n');
            } else {
                output.push(' ');
            }
            continue;
        }
        if block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                output.push_str("  ");
                chars.next();
                block_comment = false;
            } else {
                output.push(if ch == '\n' { '\n' } else { ' ' });
            }
            continue;
        }
        if let Some(active_quote) = quote {
            output.push(if ch == '\n' { '\n' } else { ' ' });
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            output.push_str("  ");
            chars.next();
            line_comment = true;
        } else if ch == '/' && chars.peek() == Some(&'*') {
            output.push_str("  ");
            chars.next();
            block_comment = true;
        } else if matches!(ch, '\'' | '"' | '`') {
            output.push(' ');
            quote = Some(ch);
        } else {
            output.push(ch);
        }
    }
    output
}

fn starts_identifier(source: &str, start: usize, identifier: &str) -> bool {
    let Some(rest) = source.get(start..) else {
        return false;
    };
    if !rest.starts_with(identifier) {
        return false;
    }
    let before = source[..start].chars().next_back();
    let after = rest[identifier.len()..].chars().next();
    !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
}

fn read_identifier(value: &str) -> Option<&str> {
    let end = value
        .char_indices()
        .take_while(|(_, ch)| is_identifier_char(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .last()?;
    Some(&value[..end])
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

fn find_value_line(text: &str, field: &str, value: &str) -> Option<usize> {
    let field = format!("\"{field}\"");
    let value = format!("\"{value}\"");
    text.lines()
        .position(|line| line.contains(&field) && line.contains(&value))
        .map(|index| index + 1)
}

fn find_token_line(text: &str, token: &str) -> Option<usize> {
    text.lines()
        .position(|line| line.contains(token))
        .map(|index| index + 1)
}

fn nested_value<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |current, key| current.get(key))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        Resolution, collect_migration_history, collect_requirements, exported_handler_status,
    };

    #[test]
    fn queue_and_scheduled_handlers_are_found_on_default_export() {
        let source = r#"
            export default {
                async queue(batch, env) {},
                scheduled(controller, env, ctx) {},
            };
        "#;
        assert_eq!(
            exported_handler_status(source, "queue"),
            Resolution::Present
        );
        assert_eq!(
            exported_handler_status(source, "scheduled"),
            Resolution::Present
        );
    }

    #[test]
    fn external_durable_objects_do_not_create_local_requirements() {
        let requirements = collect_requirements(&json!({
            "durable_objects": {
                "bindings": [
                    { "name": "LOCAL", "class_name": "Local" },
                    { "name": "REMOTE", "class_name": "Remote", "script_name": "other" }
                ]
            }
        }));
        assert_eq!(
            requirements.local_durable_object_classes,
            ["Local"].into_iter().map(str::to_string).collect()
        );
    }

    #[test]
    fn migration_renames_must_trace_to_a_created_class() {
        let history = collect_migration_history(&json!({
            "migrations": [
                { "tag": "v1", "new_classes": ["Old"] },
                { "tag": "v2", "renamed_classes": [{ "from": "Old", "to": "Current" }] },
                { "tag": "v3", "renamed_classes": [{ "from": "Missing", "to": "Orphan" }] }
            ]
        }));
        assert!(history.active.contains("Current"));
        assert!(history.orphaned_renames.contains("Orphan"));
    }
}
