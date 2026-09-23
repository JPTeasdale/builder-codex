use std::collections::BTreeSet;

use crate::analysis::ImportKind;
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{has_path_segment, has_prefix, is_test_file, is_ts_source_file, join};
use crate::rules::{Report, Rule};

pub struct WorkerDbConnectionSafetyRule;

impl Rule for WorkerDbConnectionSafetyRule {
    fn id(&self) -> &'static str {
        "worker-db-connection-safety"
    }

    fn category(&self) -> &'static str {
        "database"
    }

    fn description(&self) -> &'static str {
        "requires Worker database connections to use Hyperdrive with a single postgres-js connection and confines role changes to transactions"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_worker_runtime_file(file, config) {
                continue;
            }

            check_runtime_database_url(file, report, self.id());
            check_postgres_connections(file, report, self.id());
            check_role_changes(file, report, self.id());
        }
    }
}

fn is_worker_runtime_file(file: &ProjectFile, config: &Config) -> bool {
    is_ts_source_file(&file.rel_path)
        && !file.generated
        && !is_test_file(&file.rel_path)
        && !is_tooling_path(&file.rel_path)
        && is_worker_runtime_path(&file.rel_path, config)
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

fn is_tooling_path(rel_path: &str) -> bool {
    let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    rel_path.starts_with("scripts/")
        || rel_path.contains("/scripts/")
        || rel_path.ends_with("drizzle.config.ts")
        || has_path_segment(rel_path, "migrations")
        || has_path_segment(rel_path, "migration")
        || file_name.starts_with("db-init")
        || file_name.starts_with("db-migrate")
        || file_name.starts_with("migrate")
        || file_name.starts_with("migration")
        || file_name.starts_with("seed")
}

fn check_runtime_database_url(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let source = strip_comments(&file.text);
    if let Some((index, _)) = source
        .match_indices("DATABASE_URL")
        .find(|(index, _)| is_reference_boundary(&source, *index, "DATABASE_URL".len()))
    {
        report.error(
            rule_id,
            &file.rel_path,
            Some(line_number(&source, index)),
            "Worker runtime code reads DATABASE_URL",
            "DATABASE_URL is owner/tooling-only. In Worker runtime code, accept a Hyperdrive binding from Env and pass `env.DB.connectionString` (or the project Hyperdrive binding) to postgres-js. Keep DATABASE_URL in local scripts, drizzle.config.ts, and explicit init/migration tooling; regenerate Cloudflare types with `pnpm gen:cf` after adding the binding.",
        );
    }
}

fn check_postgres_connections(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let bindings = postgres_bindings(file);
    if bindings.is_empty() {
        return;
    }
    let connection_bindings = hyperdrive_connection_bindings(&file.text);
    let max_one_bindings = max_one_option_bindings(&file.text);
    let Some(analysis) = file.ts.as_ref() else {
        return;
    };

    for call in analysis
        .calls
        .iter()
        .filter(|call| bindings.contains(&call.callee))
    {
        let connection = call.arguments.first().map(String::as_str).unwrap_or("");
        if !is_hyperdrive_connection(connection, &connection_bindings) {
            report.error(
                rule_id,
                &file.rel_path,
                Some(call.line),
                "Worker postgres-js connection does not use Hyperdrive",
                "Construct the runtime client from a Hyperdrive binding, for example `postgres(env.DB.connectionString, { max: 1 })`. Do not pass DATABASE_URL, a plain process/env URL, or an unconstrained function parameter. Owner connection strings belong only in scripts, drizzle.config.ts, and init/migration tooling.",
            );
        }

        let options = call.arguments.get(1).map(String::as_str).unwrap_or("");
        if !has_max_one(options, &max_one_bindings) {
            report.error(
                rule_id,
                &file.rel_path,
                Some(call.line),
                "Worker postgres-js connection is missing `max: 1`",
                "Pass an explicit single-connection option: `postgres(env.DB.connectionString, { max: 1 })`. Cloudflare Workers and Hyperdrive should not create a postgres-js pool per isolate/request; keep the option visible at the call or in a local constant whose object literal contains `max: 1`.",
            );
        }
    }
}

fn check_role_changes(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let source = strip_comments(&file.text);
    let role_changes = role_changes(&source);
    if role_changes.is_empty() {
        return;
    }

    let transaction_texts = file
        .ts
        .as_ref()
        .map(|analysis| {
            analysis
                .calls
                .iter()
                .filter(|call| {
                    call.callee == "transaction" || call.callee.ends_with(".transaction")
                })
                .map(|call| normalize_sql(&call.text))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for change in role_changes {
        match change.kind {
            RoleChangeKind::Session => report.error(
                rule_id,
                &file.rel_path,
                Some(change.line),
                "session-wide database role change is forbidden in Worker runtime",
                "Use `SET LOCAL ROLE <role>` inside the callback passed to `db.transaction(...)`, or `set_config('role', <role>, true)` inside that same transaction. Session-wide SET ROLE and `set_config('role', ..., false)` can leak authorization state when postgres-js connections are reused.",
            ),
            RoleChangeKind::Local => {
                let enclosed = transaction_texts
                    .iter()
                    .any(|transaction| transaction.contains(&change.normalized));
                if !enclosed {
                    report.error(
                        rule_id,
                        &file.rel_path,
                        Some(change.line),
                        "transaction-local role change is not enclosed by a transaction call",
                        "Place SET LOCAL ROLE or `set_config('role', ..., true)` directly inside the async callback passed to `db.transaction(...)`, before invoking application code. Return only the role-scoped transaction to repositories so role state cannot escape the transaction boundary.",
                    );
                }
            }
        }
    }
}

fn postgres_bindings(file: &ProjectFile) -> BTreeSet<String> {
    let Some(analysis) = file.ts.as_ref() else {
        return BTreeSet::new();
    };
    if !analysis.imports.iter().any(|import| {
        import.source == "postgres" && import.kind == ImportKind::Static && !import.type_only
    }) {
        return BTreeSet::new();
    }

    let mut bindings = BTreeSet::new();
    for quote in ['\'', '"'] {
        let needle = format!("{quote}postgres{quote}");
        for (source_index, _) in file.text.match_indices(&needle) {
            let before = &file.text[..source_index];
            let Some(import_index) = before.rfind("import") else {
                continue;
            };
            let head = before[import_index + "import".len()..].trim();
            if head.contains(';') || !head.contains("from") || head.starts_with("type ") {
                continue;
            }
            let specifier = head.split("from").next().unwrap_or_default().trim();
            if let Some(namespace) = specifier.strip_prefix("* as ") {
                let name = namespace.split_whitespace().next().unwrap_or_default();
                if is_identifier(name) {
                    bindings.insert(name.to_string());
                }
            } else {
                let name = specifier.split(',').next().unwrap_or_default().trim();
                if is_identifier(name) {
                    bindings.insert(name.to_string());
                }
            }
        }
    }
    bindings
}

fn hyperdrive_connection_bindings(text: &str) -> BTreeSet<String> {
    assignment_bindings(text, |value| value.contains(".connectionString"))
}

fn max_one_option_bindings(text: &str) -> BTreeSet<String> {
    assignment_bindings(text, |value| has_numeric_property(value, "max", 1))
}

fn assignment_bindings(text: &str, predicate: impl Fn(&str) -> bool) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let declaration = line
                .strip_prefix("const ")
                .or_else(|| line.strip_prefix("let "))?;
            let (name, value) = declaration.split_once('=')?;
            let name = name.trim();
            (is_identifier(name) && predicate(value.trim())).then_some(name.to_string())
        })
        .collect()
}

fn is_hyperdrive_connection(argument: &str, bindings: &BTreeSet<String>) -> bool {
    let argument = argument.trim();
    argument.contains(".connectionString")
        || (is_identifier(argument) && bindings.contains(argument))
}

fn has_max_one(argument: &str, bindings: &BTreeSet<String>) -> bool {
    let argument = argument.trim();
    has_numeric_property(argument, "max", 1)
        || (is_identifier(argument) && bindings.contains(argument))
}

fn has_numeric_property(text: &str, property: &str, expected: u32) -> bool {
    text.match_indices(property).any(|(index, _)| {
        if !is_reference_boundary(text, index, property.len()) {
            return false;
        }
        let mut rest = text[index + property.len()..].trim_start();
        let Some(after_colon) = rest.strip_prefix(':') else {
            return false;
        };
        rest = after_colon.trim_start();
        let expected = expected.to_string();
        rest.strip_prefix(&expected).is_some_and(|after| {
            after
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_ascii_digit() && ch != '.')
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleChangeKind {
    Session,
    Local,
}

#[derive(Debug)]
struct RoleChange {
    line: usize,
    kind: RoleChangeKind,
    normalized: String,
}

fn role_changes(text: &str) -> Vec<RoleChange> {
    let normalized = normalize_sql(text);
    let mut changes = Vec::new();
    for needle in ["SET LOCAL ROLE", "SET SESSION ROLE", "SET ROLE"] {
        for (index, _) in normalized.match_indices(needle) {
            if needle == "SET ROLE"
                && normalized[..index]
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<String>()
                    .contains("LACOL")
            {
                continue;
            }
            let original_line = normalized[..index]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            changes.push(RoleChange {
                line: original_line,
                kind: if needle == "SET LOCAL ROLE" {
                    RoleChangeKind::Local
                } else {
                    RoleChangeKind::Session
                },
                normalized: needle.to_string(),
            });
        }
    }

    for (index, call) in set_config_role_calls(text) {
        let arguments = split_top_level(&call, ',');
        let local = arguments
            .get(2)
            .is_some_and(|argument| argument.trim().starts_with("true"));
        changes.push(RoleChange {
            line: line_number(text, index),
            kind: if local {
                RoleChangeKind::Local
            } else {
                RoleChangeKind::Session
            },
            normalized: if local {
                "SET_CONFIG('ROLE',TRUE)".to_string()
            } else {
                "SET_CONFIG('ROLE',FALSE)".to_string()
            },
        });
    }
    changes.sort_by_key(|change| change.line);
    changes.dedup_by(|left, right| left.line == right.line && left.kind == right.kind);
    changes
}

fn set_config_role_calls(text: &str) -> Vec<(usize, String)> {
    let lower = text.to_ascii_lowercase();
    let mut calls = Vec::new();
    let mut offset = 0;
    while let Some(relative) = lower[offset..].find("set_config(") {
        let start = offset + relative;
        let open = start + "set_config".len();
        let Some(close) = find_matching(text, open, '(', ')') else {
            break;
        };
        let arguments = &text[open + 1..close];
        let first = split_top_level(arguments, ',').first().map(|argument| {
            argument
                .trim()
                .trim_matches(['\'', '"'])
                .to_ascii_lowercase()
        });
        if first.as_deref() == Some("role") {
            calls.push((start, arguments.to_string()));
        }
        offset = close + 1;
    }
    calls
}

fn normalize_sql(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut previous_space = false;
    for ch in text.chars() {
        if ch == '\n' {
            output.push('\n');
            previous_space = false;
        } else if ch.is_whitespace() {
            if !previous_space {
                output.push(' ');
                previous_space = true;
            }
        } else {
            output.extend(ch.to_uppercase());
            previous_space = false;
        }
    }
    output
}

fn split_top_level(text: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut paren = 0isize;
    let mut brace = 0isize;
    let mut bracket = 0isize;
    let mut string: Option<char> = None;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if let Some(quote) = string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                string = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => string = Some(ch),
            '(' => paren += 1,
            ')' => paren -= 1,
            '{' => brace += 1,
            '}' => brace -= 1,
            '[' => bracket += 1,
            ']' => bracket -= 1,
            _ if ch == separator && paren == 0 && brace == 0 && bracket == 0 => {
                parts.push(text[start..index].trim().to_string());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if start < text.len() {
        parts.push(text[start..].trim().to_string());
    }
    parts
}

fn strip_comments(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut line_comment = false;
    let mut block_comment = false;
    let mut string: Option<char> = None;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if line_comment {
            if ch == '\n' {
                line_comment = false;
                output.push(ch);
            } else {
                output.push(' ');
            }
            continue;
        }
        if block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                output.push(' ');
                output.push(' ');
                chars.next();
                block_comment = false;
            } else {
                output.push(if ch == '\n' { '\n' } else { ' ' });
            }
            continue;
        }
        if let Some(quote) = string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                string = None;
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            output.push(' ');
            output.push(' ');
            chars.next();
            line_comment = true;
        } else if ch == '/' && chars.peek() == Some(&'*') {
            output.push(' ');
            output.push(' ');
            chars.next();
            block_comment = true;
        } else {
            if matches!(ch, '\'' | '"' | '`') {
                string = Some(ch);
            }
            output.push(ch);
        }
    }
    output
}

fn find_matching(text: &str, open_index: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut string: Option<char> = None;
    let mut escaped = false;
    for (index, ch) in text
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if let Some(quote) = string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                string = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            string = Some(ch);
        } else if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn is_reference_boundary(text: &str, start: usize, len: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[start + len..].chars().next();
    before.is_none_or(|ch| !is_identifier_char(ch))
        && after.is_none_or(|ch| !is_identifier_char(ch))
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_' || ch == '$')
        && chars.all(is_identifier_char)
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

fn line_number(text: &str, byte_index: usize) -> usize {
    text[..byte_index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        RoleChangeKind, has_max_one, is_hyperdrive_connection, role_changes, set_config_role_calls,
    };

    #[test]
    fn hyperdrive_and_single_connection_options_are_required() {
        assert!(is_hyperdrive_connection(
            "env.DB.connectionString",
            &BTreeSet::new()
        ));
        assert!(!is_hyperdrive_connection(
            "env.DATABASE_URL",
            &BTreeSet::new()
        ));
        assert!(has_max_one("{ max: 1 }", &BTreeSet::new()));
        assert!(!has_max_one("{ max: 10 }", &BTreeSet::new()));
    }

    #[test]
    fn session_and_local_role_changes_are_distinguished() {
        let changes = role_changes(
            "await tx.execute(sql`SET ROLE app_user`);\nawait tx.execute(sql`SET LOCAL ROLE app_user`);",
        );
        assert!(
            changes
                .iter()
                .any(|change| change.kind == RoleChangeKind::Session)
        );
        assert!(
            changes
                .iter()
                .any(|change| change.kind == RoleChangeKind::Local)
        );
    }

    #[test]
    fn set_config_role_requires_true_local_flag() {
        assert_eq!(
            set_config_role_calls("sql`select set_config('role', ${role}, false)`").len(),
            1
        );
        let changes = role_changes("sql`select set_config('role', ${role}, false)`");
        assert_eq!(changes[0].kind, RoleChangeKind::Session);
    }
}
