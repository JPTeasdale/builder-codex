use std::collections::BTreeSet;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, join};
use crate::rules::wrangler_environment::collect_worker_env_names;
use crate::rules::{Report, Rule};
use crate::structured::parse_jsonc;

pub struct CloudflareWorkerEnvRule;

impl Rule for CloudflareWorkerEnvRule {
    fn id(&self) -> &'static str {
        "cloudflare-worker-env"
    }

    fn category(&self) -> &'static str {
        "runtime"
    }

    fn description(&self) -> &'static str {
        "enforces Cloudflare Worker Env naming and access patterns"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        check_source_env_patterns(project, config, report, self.id());
        check_worker_env_accesses(project, config, report, self.id());
    }
}

fn check_source_env_patterns(
    project: &Project,
    config: &Config,
    report: &mut Report,
    rule_id: &'static str,
) {
    for file in &project.files {
        if !is_cloudflare_src_file(&file.rel_path, config) {
            continue;
        }

        for (index, line) in file.text.lines().enumerate() {
            let line_number = index + 1;

            if contains_process_reference(line) {
                report.error(
                    rule_id,
                    &file.rel_path,
                    Some(line_number),
                    "`process` is not available in Cloudflare Workers",
                    "Reminder: this will run in a Cloudflare Worker environment and process.env is not available.",
                );
            }

            for name in env_typed_binding_names(line) {
                if name != "env" {
                    report.error(
                        rule_id,
                        &file.rel_path,
                        Some(line_number),
                        "Cloudflare Env bindings must be named `env`",
                        "Use `env: Env` or `env: Cloudflare.Env` so Worker runtime env access is consistently written as `env.MY_ENV_NAME`.",
                    );
                }
            }
        }
    }
}

fn check_worker_env_accesses(
    project: &Project,
    config: &Config,
    report: &mut Report,
    rule_id: &'static str,
) {
    for app_root in &config.worker_apps {
        let declared = declared_worker_env_names(project, app_root);
        if declared.is_empty() {
            continue;
        }

        for access in collect_env_accesses(project, app_root) {
            if declared.contains(&access.name) {
                continue;
            }

            report.error(
                rule_id,
                access.path,
                Some(access.line),
                format!("`env.{}` is not declared in wrangler.jsonc", access.name),
                "Declare Worker runtime vars, secrets, and bindings in wrangler.jsonc so `env.MY_ENV_NAME` access matches the Cloudflare runtime surface.",
            );
        }
    }
}

fn declared_worker_env_names(project: &Project, app_root: &str) -> BTreeSet<String> {
    let wrangler_path = join(app_root, "wrangler.jsonc");
    let Some(text) = project.read(&wrangler_path) else {
        return BTreeSet::new();
    };
    parse_jsonc(text)
        .map(|wrangler| collect_worker_env_names(&wrangler))
        .unwrap_or_default()
}

#[derive(Debug)]
struct EnvAccess {
    name: String,
    path: String,
    line: usize,
}

fn collect_env_accesses(project: &Project, app_root: &str) -> Vec<EnvAccess> {
    let mut accesses = Vec::new();
    for file in &project.files {
        if !has_prefix(&file.rel_path, app_root, "src/") {
            continue;
        }

        for (index, line) in file.text.lines().enumerate() {
            collect_line_env_accesses(line, &file.rel_path, index + 1, &mut accesses);
        }
    }
    accesses
}

fn collect_line_env_accesses(
    line: &str,
    path: &str,
    line_number: usize,
    accesses: &mut Vec<EnvAccess>,
) {
    let Some(code) = strip_line_comment(line) else {
        return;
    };

    let mut offset = 0;
    while let Some(index) = code[offset..].find("env.") {
        let start = offset + index + "env.".len();
        if !is_env_object_reference(code, offset + index) {
            offset = start;
            continue;
        }

        if let Some(name) = read_env_name(&code[start..]) {
            accesses.push(EnvAccess {
                name,
                path: path.to_string(),
                line: line_number,
            });
        }
        offset = start;
    }
}

fn contains_process_reference(line: &str) -> bool {
    let mut offset = 0;
    while let Some(index) = line[offset..].find("process") {
        let start = offset + index;
        let end = start + "process".len();
        if is_identifier_boundary(line, start, end) {
            return true;
        }
        offset = end;
    }
    false
}

fn is_identifier_boundary(text: &str, start: usize, end: usize) -> bool {
    let before = if start == 0 {
        None
    } else {
        text[..start].chars().next_back()
    };
    let after = text[end..].chars().next();
    !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn env_typed_binding_names(line: &str) -> Vec<String> {
    let Some(code) = strip_line_comment(line) else {
        return Vec::new();
    };

    let mut names = Vec::new();
    for (index, ch) in code.char_indices() {
        if ch != ':' {
            continue;
        }

        let type_text = code[index + ch.len_utf8()..].trim_start();
        if !starts_with_env_type(type_text) {
            continue;
        }

        if let Some(name) = read_identifier_before(&code[..index]) {
            names.push(name);
        }
    }

    names
}

fn starts_with_env_type(text: &str) -> bool {
    starts_with_type_name(text, "Env") || starts_with_type_name(text, "Cloudflare.Env")
}

fn starts_with_type_name(text: &str, type_name: &str) -> bool {
    let Some(rest) = text.strip_prefix(type_name) else {
        return false;
    };
    rest.chars()
        .next()
        .is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'))
}

fn read_identifier_before(text: &str) -> Option<String> {
    let mut trimmed = text.trim_end();
    trimmed = trimmed.strip_suffix('?').unwrap_or(trimmed).trim_end();

    let name = trimmed
        .chars()
        .rev()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();

    if name.is_empty() { None } else { Some(name) }
}

fn is_env_object_reference(code: &str, index: usize) -> bool {
    if index > 0 {
        let Some(previous) = code[..index].chars().next_back() else {
            return true;
        };
        if previous.is_ascii_alphanumeric() || previous == '_' || previous == '.' {
            return false;
        }
    }
    true
}

fn read_env_name(text: &str) -> Option<String> {
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

fn strip_line_comment(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return None;
    }
    Some(line.split_once("//").map_or(line, |(code, _)| code))
}

fn is_cloudflare_src_file(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .chain(config.worker_apps.iter())
        .chain(config.shared_packages.iter())
        .any(|root| has_prefix(rel_path, root, "src/"))
}
