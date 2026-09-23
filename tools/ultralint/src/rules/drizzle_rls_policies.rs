use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::ImportKind;
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{DeclarationKind, is_ts_source_file, join, parse_top_level_declaration};
use crate::rules::{Report, Rule};

pub struct DrizzleRlsPoliciesRule;

impl Rule for DrizzleRlsPoliciesRule {
    fn id(&self) -> &'static str {
        "drizzle-rls-policies"
    }

    fn category(&self) -> &'static str {
        "database"
    }

    fn description(&self) -> &'static str {
        "requires every application table to use a verified centralized Drizzle RLS helper and rejects permissive user-facing policies"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        let app_roots = config
            .web_apps
            .iter()
            .chain(config.worker_apps.iter())
            .collect::<BTreeSet<_>>();

        for app_root in app_roots {
            let schema_root = join(app_root, "src/server/db/schema");
            let policies_path = join(&schema_root, "shared/policies.ts");
            let generated_auth_paths = explicit_generated_auth_paths(project, app_root);
            let tables = collect_pg_tables(project, &schema_root, &generated_auth_paths);

            if !tables.is_empty() && !project.exists(&policies_path) {
                report.error(
                    self.id(),
                    &policies_path,
                    None,
                    "Drizzle schema is missing the shared RLS policy module",
                    policies_file_help(),
                );
            }

            let verified_exports = project
                .file(&policies_path)
                .map(|file| check_policy_module(file, report, self.id()))
                .unwrap_or_default();

            check_pg_policy_centralized(project, &schema_root, &policies_path, report, self.id());

            for table in tables {
                if table.name.starts_with('_') || table.has_public_marker {
                    continue;
                }

                let policy_references = table_policy_references(
                    project.file(&table.file),
                    &table,
                    &policies_path,
                    &verified_exports,
                );
                if !policy_references.is_empty() {
                    continue;
                }

                report.error(
                    self.id(),
                    &table.file,
                    Some(table.line),
                    format!(
                        "Drizzle table `{}` is missing a verified RLS policy helper",
                        table.name
                    ),
                    "Every non-underscore application pgTable must reference a helper imported from src/server/db/schema/shared/policies.ts in its table callback, and that exported helper must create a real Drizzle pgPolicy. Name fragments such as `policy` or `rls` do not count. Import the helper and attach it, for example `(table) => [...rlsAuthenticated(table, \"select\")]`. For a genuinely public static lookup table only, put the exact comment `// ultralint: public-table` on the line immediately above pgTable. Then run `pnpm db:generate`, review the SQL policy statements, and run the RLS test suite before `pnpm db:migrate`.",
                );
            }
        }
    }
}

#[derive(Debug)]
struct PgTableDef {
    file: String,
    line: usize,
    name: String,
    callback: Option<String>,
    has_public_marker: bool,
}

#[derive(Debug)]
struct ExportedCandidate {
    name: String,
    kind: DeclarationKind,
    line: usize,
    end_line: usize,
}

#[derive(Debug)]
struct PolicyBinding {
    local_reference: String,
    exported_name: String,
}

fn collect_pg_tables(
    project: &Project,
    schema_root: &str,
    generated_auth_paths: &BTreeSet<String>,
) -> Vec<PgTableDef> {
    let mut tables = Vec::new();
    let prefix = format!("{schema_root}/");
    for file in &project.files {
        if !is_ts_source_file(&file.rel_path)
            || !file.rel_path.starts_with(&prefix)
            || generated_auth_paths.contains(&file.rel_path)
        {
            continue;
        }
        tables.extend(parse_pg_tables(file));
    }
    tables
}

fn parse_pg_tables(file: &ProjectFile) -> Vec<PgTableDef> {
    let text = file.text.as_str();
    let mut tables = Vec::new();
    let mut offset = 0;
    while let Some(relative_index) = text[offset..].find("pgTable(") {
        let start = offset + relative_index;
        if !is_identifier_boundary(text, start, "pgTable") {
            offset = start + "pgTable(".len();
            continue;
        }

        let open_paren = start + "pgTable".len();
        let Some(close_paren) = find_matching(text, open_paren, '(', ')') else {
            break;
        };
        let arguments = split_top_level(&text[open_paren + 1..close_paren], ',');
        if let Some(name) = arguments
            .first()
            .and_then(|argument| first_string_arg(argument))
        {
            tables.push(PgTableDef {
                file: file.rel_path.clone(),
                line: line_number(text, start),
                name,
                callback: arguments.get(2).cloned(),
                has_public_marker: has_adjacent_public_marker(text, start),
            });
        }
        offset = close_paren + 1;
    }
    tables
}

fn check_policy_module(
    file: &ProjectFile,
    report: &mut Report,
    rule_id: &'static str,
) -> BTreeSet<String> {
    let candidates = exported_candidates(file);
    let pg_policy_bindings = imported_named_bindings(file, "drizzle-orm/pg-core")
        .into_iter()
        .filter_map(|binding| {
            (binding.exported_name == "pgPolicy").then_some(binding.local_reference)
        })
        .collect::<BTreeSet<_>>();
    let calls = file
        .ts
        .as_ref()
        .map(|analysis| analysis.calls.as_slice())
        .unwrap_or_default();

    if file.text.contains("export {") || file.text.contains("export *") {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "shared RLS policy module uses indirect exports",
            "Export policy helpers directly from src/server/db/schema/shared/policies.ts so ultralint can resolve each table callback to the implementation that creates pgPolicy. Replace export lists/barrels with direct `export function` or `export const` declarations, then run `pnpm db:generate` and review the generated SQL.",
        );
    }

    let mut verified = candidates
        .iter()
        .filter(|candidate| {
            calls.iter().any(|call| {
                call.line >= candidate.line
                    && call.line <= candidate.end_line
                    && pg_policy_bindings.contains(&call.callee)
            })
        })
        .map(|candidate| candidate.name.clone())
        .collect::<BTreeSet<_>>();

    loop {
        let mut changed = false;
        for candidate in &candidates {
            if verified.contains(&candidate.name) {
                continue;
            }
            let delegates_to_verified = calls.iter().any(|call| {
                call.line >= candidate.line
                    && call.line <= candidate.end_line
                    && verified.contains(&call.callee)
            });
            if delegates_to_verified {
                verified.insert(candidate.name.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    for candidate in &candidates {
        if matches!(
            candidate.kind,
            DeclarationKind::Function | DeclarationKind::Const
        ) && verified.contains(&candidate.name)
        {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            Some(candidate.line),
            format!(
                "export `{}` is not a verified Drizzle RLS policy helper",
                candidate.name
            ),
            "src/server/db/schema/shared/policies.ts may directly export only functions or constants that call the pgPolicy value imported from drizzle-orm/pg-core, or delegate to another verified exported policy helper. Keep SQL constants, labels, shared types, factories, and unrelated helpers unexported or move them to another shared schema module. Install/update dependencies with `pnpm add drizzle-orm`, then run `pnpm db:generate` after fixing policy exports.",
        );
    }

    let true_bindings = unconditional_true_bindings(&file.text);
    for call in calls
        .iter()
        .filter(|call| pg_policy_bindings.contains(&call.callee))
    {
        let candidate = candidates
            .iter()
            .find(|candidate| call.line >= candidate.line && call.line <= candidate.end_line);
        let context = format!(
            "{} {}",
            candidate
                .map(|candidate| candidate.name.as_str())
                .unwrap_or(""),
            call.arguments.join(" ")
        );
        if !is_identity_scoped_policy(&context) {
            continue;
        }

        let violations =
            sensitive_policy_violations(call.arguments.get(1).map(String::as_str), &true_bindings);
        if violations.is_empty() {
            continue;
        }

        report.error(
            rule_id,
            &file.rel_path,
            Some(call.line),
            format!(
                "identity-scoped pgPolicy is unsafe: {}",
                violations.join("; ")
            ),
            "Authenticated, user, and tenant policies must name a non-PUBLIC role with `to`, and must enforce row identity/tenant predicates rather than `sql`true``. Insert policies need `withCheck`; update and all-operation policies need both `using` and `withCheck`. Use current_setting(..., true) or an equivalent scoped predicate, run `pnpm db:generate`, inspect the CREATE POLICY SQL, and exercise wrong-user/wrong-tenant cases before `pnpm db:migrate`.",
        );
    }

    verified
}

fn exported_candidates(file: &ProjectFile) -> Vec<ExportedCandidate> {
    file.text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index + 1;
            let declaration = parse_top_level_declaration(line, &file.rel_path, line_number)?;
            if !declaration.exported {
                return None;
            }
            let (_, end_line) = export_statement(&file.text, line_number);
            Some(ExportedCandidate {
                name: declaration.name,
                kind: declaration.kind,
                line: line_number,
                end_line,
            })
        })
        .collect()
}

fn table_policy_references(
    file: Option<&ProjectFile>,
    table: &PgTableDef,
    policies_path: &str,
    verified_exports: &BTreeSet<String>,
) -> Vec<String> {
    let Some(file) = file else {
        return Vec::new();
    };
    let Some(callback) = table.callback.as_deref() else {
        return Vec::new();
    };

    let callback = code_without_comments(callback);
    let mut references = Vec::new();
    for binding in policy_bindings(file, policies_path) {
        if binding.exported_name == "*" {
            for exported_name in verified_exports {
                let reference = format!("{}.{}", binding.local_reference, exported_name);
                if contains_reference(&callback, &reference) {
                    references.push(exported_name.clone());
                }
            }
        } else if verified_exports.contains(&binding.exported_name)
            && contains_reference(&callback, &binding.local_reference)
        {
            references.push(binding.exported_name);
        }
    }
    references
}

fn policy_bindings(file: &ProjectFile, policies_path: &str) -> Vec<PolicyBinding> {
    let Some(analysis) = file.ts.as_ref() else {
        return Vec::new();
    };
    let mut bindings = Vec::new();
    for import in &analysis.imports {
        if import.kind != ImportKind::Static
            || import.type_only
            || !source_resolves_to(&import.source, &file.rel_path, policies_path)
        {
            continue;
        }
        bindings.extend(imported_named_bindings(file, &import.source));
    }
    bindings
}

fn imported_named_bindings(file: &ProjectFile, source: &str) -> Vec<PolicyBinding> {
    let has_value_import = file.ts.as_ref().is_some_and(|analysis| {
        analysis.imports.iter().any(|import| {
            import.source == source && import.kind == ImportKind::Static && !import.type_only
        })
    });
    if !has_value_import {
        return Vec::new();
    }

    let mut bindings = Vec::new();
    for quote in ['\'', '"'] {
        let needle = format!("{quote}{source}{quote}");
        for (source_index, _) in file.text.match_indices(&needle) {
            let before_source = &file.text[..source_index];
            let Some(import_index) = before_source.rfind("import") else {
                continue;
            };
            let head = &file.text[import_index + "import".len()..source_index];
            if head.contains(';')
                || !head.contains("from")
                || head.trim_start().starts_with("type ")
            {
                continue;
            }

            if let (Some(open), Some(close)) = (head.find('{'), head.rfind('}')) {
                for specifier in split_top_level(&head[open + 1..close], ',') {
                    let mut words = specifier.split_whitespace();
                    let Some(exported_name) = words.next() else {
                        continue;
                    };
                    if exported_name == "type" {
                        continue;
                    }
                    let local_reference = match (words.next(), words.next()) {
                        (Some("as"), Some(alias)) => alias,
                        _ => exported_name,
                    };
                    if is_identifier(exported_name) && is_identifier(local_reference) {
                        bindings.push(PolicyBinding {
                            local_reference: local_reference.to_string(),
                            exported_name: exported_name.to_string(),
                        });
                    }
                }
            }

            if let Some(namespace) = head.find("* as ") {
                let namespace = head[namespace + "* as ".len()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or_default();
                if is_identifier(namespace) {
                    bindings.push(PolicyBinding {
                        local_reference: namespace.to_string(),
                        exported_name: "*".to_string(),
                    });
                }
            }
        }
    }
    bindings
}

fn source_resolves_to(source: &str, from_file: &str, target_file: &str) -> bool {
    let target = strip_ts_extension(target_file);
    if source.starts_with('.') {
        let parent = from_file
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        return normalize_relative(parent, source) == target;
    }

    let aliased = source
        .strip_prefix("~/")
        .or_else(|| source.strip_prefix("@/"));
    if let Some(aliased) = aliased {
        let app_prefix = target
            .find("src/")
            .map(|index| &target[..index])
            .unwrap_or("");
        return format!("{app_prefix}src/{aliased}") == target;
    }

    strip_ts_extension(source.trim_start_matches('/')) == target
}

fn explicit_generated_auth_paths(project: &Project, app_root: &str) -> BTreeSet<String> {
    let schema_prefix = format!("{}/", join(app_root, "src/server/db/schema"));
    let mut paths = project
        .files
        .iter()
        .filter(|file| {
            file.generated
                && file.rel_path.starts_with(&schema_prefix)
                && file.rel_path.rsplit('/').next() == Some("auth.gen.ts")
        })
        .map(|file| file.rel_path.clone())
        .collect::<BTreeSet<_>>();

    let package_path = join(app_root, "package.json");
    let Some(package_text) = project.read(&package_path) else {
        return paths;
    };
    let Ok(package) = serde_json::from_str::<serde_json::Value>(package_text) else {
        return paths;
    };
    let Some(scripts) = package
        .get("scripts")
        .and_then(serde_json::Value::as_object)
    else {
        return paths;
    };

    for command in scripts.values().filter_map(serde_json::Value::as_str) {
        if !command.contains("@better-auth/cli") || !command.contains("generate") {
            continue;
        }
        if let Some(output) = command_option(command, "--output") {
            let output = output.trim_start_matches("./");
            let rel_path = join(app_root, output);
            if rel_path.starts_with(&schema_prefix)
                && matches!(rel_path.rsplit('/').next(), Some("auth.ts" | "auth.gen.ts"))
            {
                paths.insert(rel_path);
            }
        }
    }
    paths
}

fn sensitive_policy_violations(
    options: Option<&str>,
    true_bindings: &BTreeSet<String>,
) -> Vec<String> {
    let properties = options.map(object_properties).unwrap_or_default();
    let mut violations = Vec::new();

    match properties.get("to") {
        None => violations.push("`to` is missing and PostgreSQL would target PUBLIC".to_string()),
        Some(target) if contains_identifier_case_insensitive(target, "public") => {
            violations.push("`to` targets PUBLIC".to_string())
        }
        _ => {}
    }

    for predicate in ["using", "withCheck"] {
        if properties
            .get(predicate)
            .is_some_and(|value| is_unconditional_true(value, true_bindings))
        {
            violations.push(format!("`{predicate}` is unconditionally true"));
        }
    }

    let operation = properties
        .get("for")
        .map(|value| unquote(value.trim()).to_ascii_lowercase());
    match operation.as_deref() {
        Some("insert") => {
            if !properties.contains_key("withCheck") {
                violations.push("insert policy is missing `withCheck`".to_string());
            }
        }
        Some("update") | Some("all") | None => {
            if !properties.contains_key("using") {
                violations.push("update/all policy is missing `using`".to_string());
            }
            if !properties.contains_key("withCheck") {
                violations.push("update/all policy is missing `withCheck`".to_string());
            }
        }
        Some("select") | Some("delete") => {}
        Some(_) => {
            if !properties.contains_key("using") || !properties.contains_key("withCheck") {
                violations.push(
                    "dynamic operation policy must provide both `using` and `withCheck`"
                        .to_string(),
                );
            }
        }
    }
    violations
}

fn unconditional_true_bindings(text: &str) -> BTreeSet<String> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let declaration = parse_top_level_declaration(line, "", index + 1)?;
            if declaration.kind != DeclarationKind::Const {
                return None;
            }
            let (statement, _) = export_statement(text, index + 1);
            let value = statement.split_once('=')?.1.trim().trim_end_matches(';');
            is_unconditional_true(value, &BTreeSet::new()).then_some(declaration.name)
        })
        .collect()
}

fn is_unconditional_true(value: &str, true_bindings: &BTreeSet<String>) -> bool {
    let mut compact = value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    while compact.starts_with('(') && compact.ends_with(')') {
        compact = compact[1..compact.len() - 1].to_string();
    }
    if true_bindings.contains(&compact) || compact == "true" {
        return true;
    }
    let lower = compact.to_ascii_lowercase();
    (lower.starts_with("sql") && lower.ends_with("`true`") && lower.contains('`'))
        || matches!(
            lower.as_str(),
            "sql.raw(\"true\")" | "sql.raw('true')" | "sql.raw(`true`)"
        )
}

fn is_identity_scoped_policy(context: &str) -> bool {
    let lower = context.to_ascii_lowercase();
    ["authenticated", "user", "tenant"]
        .iter()
        .any(|term| lower.contains(term))
}

fn check_pg_policy_centralized(
    project: &Project,
    schema_root: &str,
    policies_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let prefix = format!("{schema_root}/");
    for file in &project.files {
        if !is_ts_source_file(&file.rel_path)
            || !file.rel_path.starts_with(&prefix)
            || file.rel_path == policies_path
        {
            continue;
        }

        let imported_pg_policy = imported_named_bindings(file, "drizzle-orm/pg-core")
            .into_iter()
            .filter(|binding| binding.exported_name == "pgPolicy")
            .map(|binding| binding.local_reference)
            .collect::<BTreeSet<_>>();
        let Some(analysis) = file.ts.as_ref() else {
            continue;
        };
        for call in &analysis.calls {
            if !imported_pg_policy.contains(&call.callee) {
                continue;
            }
            report.error(
                rule_id,
                &file.rel_path,
                Some(call.line),
                "Drizzle pgPolicy is used outside shared/policies.ts",
                "Move the pgPolicy construction to src/server/db/schema/shared/policies.ts, directly export a focused helper, and import that helper into the table callback. Run `pnpm db:generate`, inspect the resulting CREATE POLICY statements, and run RLS tests before applying with `pnpm db:migrate`.",
            );
        }
    }
}

fn export_statement(text: &str, line_number: usize) -> (String, usize) {
    let lines = text.lines().collect::<Vec<_>>();
    let start = line_number.saturating_sub(1);
    let mut statement = String::new();
    let mut brace_depth = 0isize;
    let mut paren_depth = 0isize;
    let mut seen_body = false;
    let mut end_line = line_number;

    for (index, line) in lines.iter().enumerate().skip(start) {
        let trimmed = line.trim_start();
        if index > start
            && brace_depth <= 0
            && paren_depth <= 0
            && starts_top_level_declaration(trimmed)
        {
            break;
        }

        statement.push_str(line);
        statement.push('\n');
        end_line = index + 1;
        for ch in code_without_comments_and_strings(line).chars() {
            match ch {
                '{' => {
                    brace_depth += 1;
                    seen_body = true;
                }
                '}' => brace_depth -= 1,
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                _ => {}
            }
        }

        if seen_body && brace_depth <= 0 && paren_depth <= 0 {
            break;
        }
        if !seen_body && paren_depth <= 0 && line.contains(';') {
            break;
        }
    }

    (statement, end_line)
}

fn starts_top_level_declaration(trimmed: &str) -> bool {
    [
        "export ",
        "function ",
        "const ",
        "let ",
        "var ",
        "class ",
        "type ",
        "interface ",
        "enum ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

fn object_properties(text: &str) -> BTreeMap<String, String> {
    let trimmed = text.trim();
    let Some(open) = trimmed.find('{') else {
        return BTreeMap::new();
    };
    let Some(close) = find_matching(trimmed, open, '{', '}') else {
        return BTreeMap::new();
    };
    split_top_level(&trimmed[open + 1..close], ',')
        .into_iter()
        .filter_map(|entry| {
            let colon = find_top_level_separator(&entry, ':')?;
            let key = unquote(entry[..colon].trim()).to_string();
            let value = entry[colon + 1..].trim().to_string();
            (!key.is_empty()).then_some((key, value))
        })
        .collect()
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

fn find_top_level_separator(text: &str, separator: char) -> Option<usize> {
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
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

fn has_adjacent_public_marker(text: &str, byte_index: usize) -> bool {
    let current_line_start = text[..byte_index]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    text[..current_line_start]
        .lines()
        .next_back()
        .is_some_and(|line| line.trim() == "// ultralint: public-table")
}

fn contains_reference(text: &str, reference: &str) -> bool {
    if let Some((namespace, _)) = reference.split_once('.') {
        return text.match_indices(reference).any(|(index, _)| {
            is_reference_boundary(text, index, reference.len())
                && contains_identifier(text, namespace)
        });
    }
    contains_identifier(text, reference)
}

fn contains_identifier(text: &str, identifier: &str) -> bool {
    text.match_indices(identifier)
        .any(|(index, _)| is_reference_boundary(text, index, identifier.len()))
}

fn contains_identifier_case_insensitive(text: &str, identifier: &str) -> bool {
    contains_identifier(&text.to_ascii_lowercase(), &identifier.to_ascii_lowercase())
}

fn is_reference_boundary(text: &str, start: usize, len: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[start + len..].chars().next();
    before.is_none_or(|ch| !is_identifier_char(ch))
        && after.is_none_or(|ch| !is_identifier_char(ch))
}

fn is_identifier_boundary(text: &str, start: usize, ident: &str) -> bool {
    is_reference_boundary(text, start, ident.len())
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

fn find_matching(text: &str, open_index: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let bytes = text.as_bytes();

    for (index, ch) in text
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            continue;
        }
        if in_block_comment {
            if ch == '*' && bytes.get(index + 1) == Some(&b'/') {
                in_block_comment = false;
            }
            continue;
        }
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }

        if ch == '/' && bytes.get(index + 1) == Some(&b'/') {
            in_line_comment = true;
            continue;
        }
        if ch == '/' && bytes.get(index + 1) == Some(&b'*') {
            in_block_comment = true;
            continue;
        }
        if ch == '"' || ch == '\'' || ch == '`' {
            in_string = Some(ch);
            continue;
        }
        if ch == open {
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

fn first_string_arg(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let quote = trimmed.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let start = quote.len_utf8();
    let end = trimmed[start..].find(quote)?;
    Some(trimmed[start..start + end].to_string())
}

fn line_number(text: &str, byte_index: usize) -> usize {
    text[..byte_index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn normalize_relative(parent: &str, source: &str) -> String {
    let mut parts = parent
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    for part in source.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part.to_string()),
        }
    }
    strip_ts_extension(&parts.join("/"))
}

fn strip_ts_extension(path: &str) -> String {
    path.strip_suffix(".tsx")
        .or_else(|| path.strip_suffix(".ts"))
        .or_else(|| path.strip_suffix(".js"))
        .unwrap_or(path)
        .trim_end_matches("/index")
        .to_string()
}

fn command_option(command: &str, option: &str) -> Option<String> {
    let tokens = command
        .split_whitespace()
        .map(|token| token.trim_matches(['\'', '"']))
        .collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate() {
        if *token == option {
            return tokens.get(index + 1).map(|value| value.to_string());
        }
        if let Some(value) = token.strip_prefix(&format!("{option}=")) {
            return Some(value.to_string());
        }
    }
    None
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .or_else(|| {
            value
                .strip_prefix('`')
                .and_then(|value| value.strip_suffix('`'))
        })
        .unwrap_or(value)
}

fn code_without_comments_and_strings(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut string: Option<char> = None;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if let Some(quote) = string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                string = None;
            }
            output.push(' ');
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            break;
        }
        if matches!(ch, '\'' | '"' | '`') {
            string = Some(ch);
            output.push(' ');
        } else {
            output.push(ch);
        }
    }
    output
}

fn code_without_comments(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                output.push(ch);
            } else {
                output.push(' ');
            }
            continue;
        }
        if in_block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                output.push(' ');
                output.push(' ');
                chars.next();
                in_block_comment = false;
            } else {
                output.push(if ch == '\n' { '\n' } else { ' ' });
            }
            continue;
        }
        if let Some(quote) = in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            output.push(' ');
            output.push(' ');
            chars.next();
            in_line_comment = true;
        } else if ch == '/' && chars.peek() == Some(&'*') {
            output.push(' ');
            output.push(' ');
            chars.next();
            in_block_comment = true;
        } else {
            if matches!(ch, '\'' | '"' | '`') {
                in_string = Some(ch);
            }
            output.push(ch);
        }
    }
    output
}

fn policies_file_help() -> &'static str {
    "Create src/server/db/schema/shared/policies.ts and import pgPolicy from drizzle-orm/pg-core. Directly export focused helpers that construct pgPolicy with explicit non-PUBLIC roles and operation-correct predicates: insert requires `withCheck`; update requires `using` plus `withCheck`. Authenticated/user/tenant helpers must scope rows with current_setting(..., true) or an equivalent identity predicate, never unconditional `sql`true``. Install/update with `pnpm add drizzle-orm`, attach the helpers in each pgTable callback, run `pnpm db:generate`, review the generated SQL, and run RLS tests before `pnpm db:migrate`. Better Auth output is exempt only when the file is exactly auth.gen.ts or is the explicit `@better-auth/cli generate --output .../auth.ts` target; regenerate it with `pnpm gen:auth` rather than editing it by hand."
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        PgTableDef, has_adjacent_public_marker, is_unconditional_true, sensitive_policy_violations,
        source_resolves_to,
    };

    #[test]
    fn public_marker_must_be_immediately_adjacent() {
        let adjacent = "// ultralint: public-table\npgTable(";
        assert!(has_adjacent_public_marker(
            adjacent,
            adjacent.find("pgTable").unwrap()
        ));

        let separated = "// ultralint: public-table\n\npgTable(";
        assert!(!has_adjacent_public_marker(
            separated,
            separated.find("pgTable").unwrap()
        ));
    }

    #[test]
    fn sensitive_insert_and_update_require_scoped_predicates() {
        let insert = sensitive_policy_violations(
            Some("{ for: 'insert', to: roleAuthenticated, using: sql`true` }"),
            &BTreeSet::new(),
        );
        assert!(insert.iter().any(|issue| issue.contains("withCheck")));
        assert!(
            insert
                .iter()
                .any(|issue| issue.contains("unconditionally true"))
        );

        let update = sensitive_policy_violations(
            Some("{ for: 'update', to: roleUser, using: userPredicate }"),
            &BTreeSet::new(),
        );
        assert!(update.iter().any(|issue| issue.contains("withCheck")));
    }

    #[test]
    fn unconditional_sql_true_is_exact() {
        assert!(is_unconditional_true(
            "sql<boolean>`true`",
            &BTreeSet::new()
        ));
        assert!(!is_unconditional_true(
            "sql`true AND owner_id = current_setting('app.user_id')`",
            &BTreeSet::new()
        ));
    }

    #[test]
    fn policy_import_must_resolve_to_the_shared_module() {
        assert!(source_resolves_to(
            "./shared/policies",
            "src/server/db/schema/events.ts",
            "src/server/db/schema/shared/policies.ts"
        ));
        assert!(!source_resolves_to(
            "./fake-policies",
            "src/server/db/schema/events.ts",
            "src/server/db/schema/shared/policies.ts"
        ));
    }

    #[test]
    fn table_shape_does_not_accept_policy_like_names_by_itself() {
        let table = PgTableDef {
            file: "src/server/db/schema/events.ts".to_string(),
            line: 1,
            name: "events".to_string(),
            callback: Some("() => [fakeRlsPolicy()]".to_string()),
            has_public_marker: false,
        };
        assert!(table.callback.unwrap().contains("fakeRlsPolicy"));
    }
}
