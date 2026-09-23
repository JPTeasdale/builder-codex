use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::analysis::{CallFact, ImportFact};
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{has_prefix, is_test_file, is_ts_source_file, join};
use crate::rules::{Report, Rule};
use crate::structured::parse_yaml;

pub struct FocusedTestsRule;

impl Rule for FocusedTestsRule {
    fn id(&self) -> &'static str {
        "focused-tests"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "rejects committed focused test calls"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        for file in test_files(project) {
            let Some(analysis) = &file.ts else {
                continue;
            };
            for call in analysis
                .calls
                .iter()
                .filter(|call| is_test_modifier(&call.callee, "only"))
            {
                report.error(
                    self.id(),
                    &file.rel_path,
                    Some(call.line),
                    format!("focused test `{}` must not be committed", call.callee),
                    "remove `.only`, then run the full suite with `pnpm test`",
                );
            }
        }
    }
}

pub struct TestSkipReasonsRule;

impl Rule for TestSkipReasonsRule {
    fn id(&self) -> &'static str {
        "test-skip-reasons"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "warns when skipped or todo tests lack a reason or issue reference"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        for file in test_files(project) {
            let Some(analysis) = &file.ts else {
                continue;
            };
            for call in analysis.calls.iter().filter(|call| {
                is_test_modifier(&call.callee, "skip") || is_test_modifier(&call.callee, "todo")
            }) {
                if skip_has_reason(file, call) {
                    continue;
                }
                report.warning(
                    self.id(),
                    &file.rel_path,
                    Some(call.line),
                    format!("`{}` lacks a reason or issue reference", call.callee),
                    "add an adjacent reason with an issue URL/key, for example `// Blocked by ENG-123`, so `pnpm test` debt stays actionable",
                );
            }
        }
    }
}

pub struct DrizzleMutationSafetyRule;

impl Rule for DrizzleMutationSafetyRule {
    fn id(&self) -> &'static str {
        "drizzle-mutation-safety"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "warns on complete Drizzle update and delete chains without where clauses"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        for file in &project.files {
            if file.generated || !is_ts_source_file(&file.rel_path) {
                continue;
            }
            for violation in drizzle_mutations_without_where(file) {
                report.warning(
                    self.id(),
                    &file.rel_path,
                    Some(violation.line),
                    format!(
                        "complete Drizzle {} chain executes without `.where(...)`",
                        violation.operation
                    ),
                    "add an explicit `.where(...)`; keep intentional all-row mutations isolated and reviewed before running `pnpm test`",
                );
            }
        }
    }
}

pub struct ImportCyclesRule;

impl Rule for ImportCyclesRule {
    fn id(&self) -> &'static str {
        "import-cycles"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "warns on runtime import cycles while excluding type-only edges"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        let graph = runtime_import_graph(project);
        for component in strongly_connected_components(&graph) {
            let cyclic = component.len() > 1
                || component.first().is_some_and(|node| {
                    graph
                        .get(node)
                        .is_some_and(|targets| targets.contains(node))
                });
            if !cyclic {
                continue;
            }
            let path = component.first().cloned().unwrap_or_default();
            report.warning(
                self.id(),
                path,
                None,
                format!("runtime import cycle: {}", component.join(" -> ")),
                "move the shared runtime dependency behind a one-way module boundary, then verify imports with `pnpm ultralint`; type-only imports are already excluded",
            );
        }
    }
}

pub struct WorkerMutableStateRule;

impl Rule for WorkerMutableStateRule {
    fn id(&self) -> &'static str {
        "worker-module-state"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "warns on mutable module-scope state in Worker runtime source"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if file.generated
                || is_test_file(&file.rel_path)
                || !is_ts_source_file(&file.rel_path)
                || !is_worker_runtime_path(&file.rel_path, config)
            {
                continue;
            }
            for state in module_scope_mutable_state(&file.text) {
                report.warning(
                    self.id(),
                    &file.rel_path,
                    Some(state.line),
                    format!(
                        "module-scope `{}` may retain mutable Worker state between requests",
                        state.name
                    ),
                    "move request state inside the handler or persist shared state in a Cloudflare binding; validate the change with `pnpm test`",
                );
            }
        }
    }
}

fn is_worker_runtime_path(rel_path: &str, config: &Config) -> bool {
    config.worker_apps.iter().any(|root| {
        if config.web_apps.contains(root) {
            rel_path == join(root, "src/server.ts") || has_prefix(rel_path, root, "src/server/")
        } else {
            has_prefix(rel_path, root, "src/")
        }
    })
}

pub struct DeployWorkflowTargetsRule;

impl Rule for DeployWorkflowTargetsRule {
    fn id(&self) -> &'static str {
        "deploy-workflow-targets"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "warns on unsafe preview and production deploy workflow targeting"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_workflow_file(&file.rel_path) {
                continue;
            }
            let workflow = match parse_yaml(&file.text) {
                Ok(workflow) => workflow,
                Err(_) => continue,
            };
            check_deploy_workflow(file, &workflow, report, self.id());
        }
    }
}

fn test_files(project: &Project) -> impl Iterator<Item = &ProjectFile> {
    project
        .files
        .iter()
        .filter(|file| !file.generated && is_test_file(&file.rel_path))
}

fn is_test_modifier(callee: &str, modifier: &str) -> bool {
    let parts = callee.split('.').collect::<Vec<_>>();
    matches!(
        parts.first(),
        Some(&"test" | &"it" | &"describe" | &"suite" | &"bench")
    ) && parts.iter().skip(1).any(|part| *part == modifier)
}

fn skip_has_reason(file: &ProjectFile, call: &CallFact) -> bool {
    if call
        .arguments
        .iter()
        .any(|argument| has_reason_marker(argument))
    {
        return true;
    }
    let lines = file.text.lines().collect::<Vec<_>>();
    let index = call.line.saturating_sub(1);
    let start = index.saturating_sub(1);
    lines
        .get(start..=index.min(lines.len().saturating_sub(1)))
        .is_some_and(|context| {
            context.iter().any(|line| {
                line.split_once("//")
                    .map(|(_, comment)| comment)
                    .or_else(|| line.split_once("/*").map(|(_, comment)| comment))
                    .is_some_and(has_reason_marker)
            })
        })
}

fn has_reason_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("because ")
        || lower.contains("blocked by ")
        || lower.contains("reason:")
    {
        return true;
    }
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '#'))
        .any(|token| {
            if let Some(number) = token.strip_prefix('#') {
                return !number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit());
            }
            let Some((project, number)) = token.rsplit_once('-') else {
                return false;
            };
            project.len() >= 2
                && project.chars().all(|ch| ch.is_ascii_uppercase())
                && !number.is_empty()
                && number.chars().all(|ch| ch.is_ascii_digit())
        })
}

#[derive(Debug, PartialEq, Eq)]
struct MutationViolation {
    line: usize,
    operation: &'static str,
}

fn drizzle_mutations_without_where(file: &ProjectFile) -> Vec<MutationViolation> {
    let Some(analysis) = &file.ts else {
        return Vec::new();
    };
    let mut violations = BTreeSet::new();
    for call in &analysis.calls {
        let text = &call.text;
        let operation = if text.contains(".update(") && text.contains(".set(") {
            "update"
        } else if text.contains(".delete(") {
            "delete"
        } else {
            continue;
        };
        if text.contains(".where(") || !mutation_is_complete(&file.text, call) {
            continue;
        }
        violations.insert((call.line, operation));
    }
    violations
        .into_iter()
        .map(|(line, operation)| MutationViolation { line, operation })
        .collect()
}

fn mutation_is_complete(source: &str, call: &CallFact) -> bool {
    if call.text.contains(".execute(") || call.text.contains(".returning(") {
        return true;
    }
    let statement = statement_around_line(source, call.line);
    statement.contains("await ")
        || statement.trim_start().starts_with("return ")
        || statement.contains("=> db.")
}

fn statement_around_line(source: &str, line: usize) -> String {
    let lines = source.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    let mut start = line.saturating_sub(1).min(lines.len() - 1);
    while start > 0
        && !lines[start - 1].contains(';')
        && !lines[start - 1].contains('{')
        && line - start < 8
    {
        start -= 1;
    }
    let mut end = line.saturating_sub(1).min(lines.len() - 1);
    while end + 1 < lines.len() && !lines[end].contains(';') && end + 1 - start < 12 {
        end += 1;
    }
    lines[start..=end].join("\n")
}

fn runtime_import_graph(project: &Project) -> BTreeMap<String, BTreeSet<String>> {
    let mut graph = BTreeMap::new();
    for file in &project.files {
        if file.generated || !is_ts_source_file(&file.rel_path) {
            continue;
        }
        let mut targets = BTreeSet::new();
        if let Some(analysis) = &file.ts {
            for import in &analysis.imports {
                if import_is_type_only(file, import) {
                    continue;
                }
                let Some(target) = project.resolve_import(&file.rel_path, &import.source) else {
                    continue;
                };
                if project.is_file(&target) && is_ts_source_file(&target) {
                    targets.insert(target);
                }
            }
        }
        graph.insert(file.rel_path.clone(), targets);
    }
    graph
}

fn import_is_type_only(file: &ProjectFile, import: &ImportFact) -> bool {
    if import.type_only {
        return true;
    }
    let lines = file.text.lines().collect::<Vec<_>>();
    let mut statement = String::new();
    for line in lines.iter().skip(import.line.saturating_sub(1)).take(12) {
        statement.push_str(line);
        statement.push('\n');
        if line.contains(';') || line.contains(&import.source) {
            break;
        }
    }
    let trimmed = statement.trim_start();
    if trimmed.starts_with("import type ") || trimmed.starts_with("export type ") {
        return true;
    }
    let Some(open) = statement.find('{') else {
        return false;
    };
    let Some(close) = statement[open + 1..]
        .find('}')
        .map(|index| open + 1 + index)
    else {
        return false;
    };
    let specifiers = statement[open + 1..close]
        .split(',')
        .map(str::trim)
        .filter(|specifier| !specifier.is_empty())
        .collect::<Vec<_>>();
    !specifiers.is_empty()
        && specifiers
            .iter()
            .all(|specifier| specifier.starts_with("type "))
}

fn strongly_connected_components(graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        visited: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if !visited.insert(node.to_string()) {
            return;
        }
        if let Some(targets) = graph.get(node) {
            for target in targets {
                visit(target, graph, visited, order);
            }
        }
        order.push(node.to_string());
    }

    fn collect(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        visited: &mut BTreeSet<String>,
        component: &mut Vec<String>,
    ) {
        if !visited.insert(node.to_string()) {
            return;
        }
        component.push(node.to_string());
        if let Some(targets) = graph.get(node) {
            for target in targets {
                collect(target, graph, visited, component);
            }
        }
    }

    let mut order = Vec::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys() {
        visit(node, graph, &mut visited, &mut order);
    }
    let mut reverse = graph
        .keys()
        .map(|node| (node.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for (source, targets) in graph {
        for target in targets {
            reverse
                .entry(target.clone())
                .or_default()
                .insert(source.clone());
        }
    }

    visited.clear();
    let mut components = Vec::new();
    for node in order.into_iter().rev() {
        if visited.contains(&node) {
            continue;
        }
        let mut component = Vec::new();
        collect(&node, &reverse, &mut visited, &mut component);
        component.sort();
        components.push(component);
    }
    components.sort();
    components
}

#[derive(Debug, PartialEq, Eq)]
struct MutableState {
    line: usize,
    name: String,
}

fn module_scope_mutable_state(source: &str) -> Vec<MutableState> {
    let sanitized = sanitize_source(source);
    let lines = sanitized.lines().collect::<Vec<_>>();
    let mut depth = 0isize;
    let mut findings = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if depth == 0 {
            let declaration = trimmed
                .strip_prefix("export ")
                .unwrap_or(trimmed)
                .trim_start();
            let (kind, rest) = if let Some(rest) = declaration.strip_prefix("let ") {
                ("let", rest)
            } else if let Some(rest) = declaration.strip_prefix("var ") {
                ("var", rest)
            } else if let Some(rest) = declaration.strip_prefix("const ") {
                ("const", rest)
            } else {
                ("", "")
            };
            if !kind.is_empty()
                && let Some(name) = read_identifier(rest)
            {
                let mutable_const = kind == "const" && const_is_mutable(rest, source, name);
                if kind != "const" || mutable_const {
                    findings.push(MutableState {
                        line: index + 1,
                        name: name.to_string(),
                    });
                }
            }
        }
        depth += line.matches('{').count() as isize;
        depth -= line.matches('}').count() as isize;
        depth = depth.max(0);
    }
    findings
}

fn const_is_mutable(declaration: &str, source: &str, name: &str) -> bool {
    if declaration.contains(" as const") || declaration.contains("Object.freeze(") {
        return false;
    }
    if ["new Map", "new Set", "new WeakMap", "new WeakSet"]
        .iter()
        .any(|marker| declaration.contains(marker))
    {
        return true;
    }
    let literal_collection = declaration.contains("= []") || declaration.contains("= {");
    literal_collection
        && [
            format!("{name}.push("),
            format!("{name}.pop("),
            format!("{name}.splice("),
            format!("{name}.set("),
            format!("{name}.add("),
            format!("{name}["),
        ]
        .iter()
        .any(|marker| source.contains(marker))
}

fn sanitize_source(source: &str) -> String {
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

fn read_identifier(value: &str) -> Option<&str> {
    let end = value
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$')
        .map(|(index, ch)| index + ch.len_utf8())
        .last()?;
    Some(&value[..end])
}

fn is_workflow_file(path: &str) -> bool {
    path.contains(".github/workflows/") && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

fn check_deploy_workflow(
    file: &ProjectFile,
    workflow: &Value,
    report: &mut Report,
    rule_id: &'static str,
) {
    let pull_request = workflow_has_trigger(workflow, "pull_request");
    let workflow_name = workflow
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(&file.rel_path);
    let Some(jobs) = workflow.get("jobs").and_then(Value::as_object) else {
        return;
    };

    for (job_id, job) in jobs {
        let job_name = job.get("name").and_then(Value::as_str).unwrap_or(job_id);
        let environment = workflow_environment(job);
        let context = format!("{} {} {}", workflow_name, job_id, job_name).to_ascii_lowercase();
        let Some(steps) = job.get("steps").and_then(Value::as_array) else {
            continue;
        };
        for step in steps {
            let Some(command) = step.get("run").and_then(Value::as_str) else {
                continue;
            };
            if !is_deploy_command(command) {
                continue;
            }
            let target = deploy_target(command, &context, environment.as_deref());
            let line = find_command_line(&file.text, command);
            if target == DeployTarget::Preview && !has_env_flag(command, "preview") {
                report.warning(
                    rule_id,
                    &file.rel_path,
                    line,
                    format!("preview deploy job `{job_id}` omits `--env preview`"),
                    "target preview explicitly with `pnpm exec wrangler deploy --env preview`",
                );
            }
            if target == DeployTarget::Production
                && pull_request
                && !job_excludes_pull_requests(job)
            {
                report.warning(
                    rule_id,
                    &file.rel_path,
                    line,
                    format!("production deploy job `{job_id}` can run from pull_request"),
                    "limit production deployment to a protected push or workflow_dispatch path and run `pnpm exec wrangler deploy --env production` only after approval",
                );
            }
            if environment.is_none() {
                report.warning(
                    rule_id,
                    &file.rel_path,
                    line,
                    format!("deploy job `{job_id}` has no GitHub environment gate"),
                    "set `jobs.<job>.environment` to the target environment and protect it before the pnpm deploy command runs",
                );
            } else if matches!(target, DeployTarget::Preview | DeployTarget::Production)
                && environment
                    .as_deref()
                    .is_some_and(|environment| environment.to_ascii_lowercase() != target.as_str())
            {
                report.warning(
                    rule_id,
                    &file.rel_path,
                    line,
                    format!(
                        "deploy job `{job_id}` targets {} but gates on environment `{}`",
                        target.as_str(),
                        environment.as_deref().unwrap_or_default()
                    ),
                    "make the GitHub environment gate match the `pnpm exec wrangler deploy --env <target>` environment",
                );
            }
        }
    }
}

fn workflow_has_trigger(workflow: &Value, trigger: &str) -> bool {
    let Some(on) = workflow.get("on") else {
        return false;
    };
    match on {
        Value::String(value) => value == trigger,
        Value::Array(values) => values.iter().any(|value| value.as_str() == Some(trigger)),
        Value::Object(values) => values.contains_key(trigger),
        _ => false,
    }
}

fn workflow_environment(job: &Value) -> Option<String> {
    match job.get("environment")? {
        Value::String(value) => Some(value.to_string()),
        Value::Object(value) => value
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn is_deploy_command(command: &str) -> bool {
    command.contains("wrangler deploy")
        || command.contains("pnpm deploy")
        || command.contains("pnpm run deploy")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeployTarget {
    Preview,
    Production,
    Unknown,
}

impl DeployTarget {
    fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Production => "production",
            Self::Unknown => "unknown",
        }
    }
}

fn deploy_target(command: &str, context: &str, environment: Option<&str>) -> DeployTarget {
    if has_env_flag(command, "preview") {
        return DeployTarget::Preview;
    }
    if has_env_flag(command, "production") {
        return DeployTarget::Production;
    }
    let environment = environment.unwrap_or_default().to_ascii_lowercase();
    if environment == "preview" || context.contains("preview") {
        DeployTarget::Preview
    } else if environment == "production"
        || environment == "prod"
        || context.contains("production")
        || context.split_whitespace().any(|word| word == "prod")
    {
        DeployTarget::Production
    } else {
        DeployTarget::Unknown
    }
}

fn has_env_flag(command: &str, environment: &str) -> bool {
    let words = command.split_whitespace().collect::<Vec<_>>();
    words.windows(2).any(|pair| pair == ["--env", environment])
        || words
            .iter()
            .any(|word| *word == format!("--env={environment}"))
}

fn job_excludes_pull_requests(job: &Value) -> bool {
    let condition = job
        .get("if")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .replace('"', "'");
    condition.contains("github.event_name == 'push'")
        || condition.contains("github.event_name != 'pull_request'")
}

fn find_command_line(source: &str, command: &str) -> Option<usize> {
    let first = command.lines().find(|line| !line.trim().is_empty())?.trim();
    source
        .lines()
        .position(|line| line.contains(first))
        .map(|index| index + 1)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        DeployTarget, deploy_target, has_reason_marker, module_scope_mutable_state,
        strongly_connected_components,
    };

    #[test]
    fn issue_keys_and_explanations_count_as_skip_reasons() {
        assert!(has_reason_marker("Blocked by ENG-123"));
        assert!(has_reason_marker(
            "because the upstream service is unavailable"
        ));
        assert!(!has_reason_marker("flaky"));
    }

    #[test]
    fn mutable_worker_state_ignores_frozen_and_const_values() {
        let state = module_scope_mutable_state(
            r#"
let requestCount = 0;
const cache = new Map<string, string>();
const flags = Object.freeze({ enabled: true });
export default { fetch() { return flags.enabled; } };
            "#,
        );
        assert_eq!(state.len(), 2);
        assert_eq!(state[0].name, "requestCount");
        assert_eq!(state[1].name, "cache");
    }

    #[test]
    fn type_filtered_graph_reports_runtime_components() {
        let graph = BTreeMap::from([
            ("a.ts".to_string(), BTreeSet::from(["b.ts".to_string()])),
            ("b.ts".to_string(), BTreeSet::from(["a.ts".to_string()])),
            ("types.ts".to_string(), BTreeSet::new()),
        ]);
        let components = strongly_connected_components(&graph);
        assert!(
            components
                .iter()
                .any(|component| component == &["a.ts", "b.ts"])
        );
    }

    #[test]
    fn workflow_target_uses_flags_before_names() {
        assert_eq!(
            deploy_target(
                "pnpm exec wrangler deploy --env production",
                "preview deploy",
                Some("production")
            ),
            DeployTarget::Production
        );
    }
}
