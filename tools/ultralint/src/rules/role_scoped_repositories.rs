use std::collections::{BTreeMap, BTreeSet};

use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{
    DeclarationKind, is_test_file, is_ts_source_file, join, parse_top_level_declaration,
};
use crate::rules::{Report, Rule};

pub struct RoleScopedRepositoriesRule;

const REQUIRED_ROLE_EXPORTS: [&str; 5] = [
    "withUserRole",
    "withServerRole",
    "UserTransaction",
    "ServerTransaction",
    "RoleTransaction",
];

const ROLE_TRANSACTION_TYPES: [&str; 3] =
    ["RoleTransaction", "UserTransaction", "ServerTransaction"];

impl Rule for RoleScopedRepositoriesRule {
    fn id(&self) -> &'static str {
        "role-scoped-repositories"
    }

    fn category(&self) -> &'static str {
        "database"
    }

    fn description(&self) -> &'static str {
        "requires repositories to expose only role-scoped transaction APIs and keeps their dependencies inside the server database boundary"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        let repository_groups = repository_groups(project);
        for (app_root, repositories) in repository_groups {
            let db_root = join(&app_root, "src/server/db");
            let roles_path = join(&db_root, "roles.ts");
            check_roles_module(project.file(&roles_path), &roles_path, report, self.id());

            for file in repositories {
                check_repository_signatures(file, report, self.id());
                check_repository_imports(file, &db_root, config, report, self.id());
            }
        }
    }
}

fn repository_groups(project: &Project) -> BTreeMap<String, Vec<&ProjectFile>> {
    let marker = "src/server/db/repositories/";
    let mut groups = BTreeMap::<String, Vec<&ProjectFile>>::new();
    for file in &project.files {
        if !is_ts_source_file(&file.rel_path) || is_test_file(&file.rel_path) {
            continue;
        }
        let Some(index) = file.rel_path.find(marker) else {
            continue;
        };
        let app_root = file.rel_path[..index].trim_end_matches('/');
        groups
            .entry(if app_root.is_empty() {
                ".".to_string()
            } else {
                app_root.to_string()
            })
            .or_default()
            .push(file);
    }
    groups
}

fn check_roles_module(
    file: Option<&ProjectFile>,
    roles_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(file) = file else {
        report.error(
            rule_id,
            roles_path,
            None,
            "repositories exist but src/server/db/roles.ts is missing",
            roles_help(),
        );
        return;
    };

    let exports = exported_names(&file.text, &file.rel_path);
    let missing = REQUIRED_ROLE_EXPORTS
        .iter()
        .filter(|name| !exports.contains(**name))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        report.error(
            rule_id,
            roles_path,
            None,
            format!(
                "role transaction module is missing exports: {}",
                missing.join(", ")
            ),
            roles_help(),
        );
    }

    if exports.contains("RoleTransaction") && !role_transaction_allows_both(&file.text) {
        report.error(
            rule_id,
            roles_path,
            declaration_line(&file.text, "RoleTransaction"),
            "RoleTransaction does not include both UserTransaction and ServerTransaction",
            "Define `export type RoleTransaction = UserTransaction | ServerTransaction` so shared repository operations can explicitly accept either role-scoped transaction without falling back to AppDb. Keep narrower operations typed to UserTransaction or ServerTransaction when only one role is valid.",
        );
    }
}

fn check_repository_signatures(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    for parameters in function_parameter_sections(&file.text) {
        let raw_types = ["ServerContext", "AppDb"]
            .into_iter()
            .filter(|name| contains_identifier(&parameters.text, name))
            .collect::<Vec<_>>();
        if raw_types.is_empty() {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            Some(parameters.line),
            format!(
                "repository function accepts forbidden raw database context: {}",
                raw_types.join(", ")
            ),
            "Repository functions must receive RoleTransaction, UserTransaction, or ServerTransaction. Open the transaction with withUserRole/withServerRole in the server DB orchestration layer, set role and identity context transaction-locally, then pass only the scoped transaction into the repository. Never pass ServerContext or raw AppDb through this boundary.",
        );
    }

    for function in exported_repository_functions(&file.text, &file.rel_path) {
        if ROLE_TRANSACTION_TYPES
            .iter()
            .any(|name| contains_identifier(&function.parameters, name))
        {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            Some(function.line),
            format!(
                "exported repository function `{}` does not accept a role transaction",
                function.name
            ),
            "Add a `tx: RoleTransaction` parameter when either user or server access is valid, or use the narrower UserTransaction/ServerTransaction type when the operation is role-specific. Callers must enter withUserRole/withServerRole before invoking the repository; do not let repositories create or recover raw AppDb connections.",
        );
    }
}

fn check_repository_imports(
    file: &ProjectFile,
    db_root: &str,
    config: &Config,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(analysis) = file.ts.as_ref() else {
        return;
    };

    for import in &analysis.imports {
        if import_allowed(
            &import.source,
            import.type_only,
            &file.rel_path,
            db_root,
            config,
        ) {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            Some(import.line),
            format!(
                "repository import `{}` crosses the server database boundary",
                import.source
            ),
            "Keep repository runtime imports under src/server/db (roles, schema, and repository helpers) plus database packages such as drizzle-orm/postgres. Shared contracts may be imported only with `import type` from a shared/contracts/types module. Move domain orchestration, ServerContext, auth, request, and service dependencies above the repository layer.",
        );
    }
}

fn import_allowed(
    source: &str,
    type_only: bool,
    from_file: &str,
    db_root: &str,
    config: &Config,
) -> bool {
    if is_database_package(source) {
        return true;
    }
    if source.starts_with('.') {
        return normalize_relative(from_file, source).starts_with(&format!("{db_root}/"))
            || normalize_relative(from_file, source) == db_root;
    }
    if source
        .strip_prefix("~/")
        .or_else(|| source.strip_prefix("@/"))
        .is_some_and(|path| path == "server/db" || path.starts_with("server/db/"))
        || source == "src/server/db"
        || source.starts_with("src/server/db/")
    {
        return true;
    }

    type_only && is_shared_contract_source(source, config)
}

fn is_database_package(source: &str) -> bool {
    source == "postgres"
        || source == "drizzle-orm"
        || source.starts_with("drizzle-orm/")
        || source.starts_with("@neondatabase/")
}

fn is_shared_contract_source(source: &str, config: &Config) -> bool {
    let normalized = source.replace(['@', '~', '.'], "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    segments
        .iter()
        .any(|segment| matches!(*segment, "shared" | "contracts" | "types"))
        || source.ends_with(".types")
        || config.shared_packages.iter().any(|root| {
            let package_name = root.rsplit('/').next().unwrap_or(root);
            source.split('/').any(|segment| segment == package_name)
        })
}

fn exported_names(text: &str, file: &str) -> BTreeSet<String> {
    let mut names = text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let declaration = parse_top_level_declaration(line, file, index + 1)?;
            declaration.exported.then_some(declaration.name)
        })
        .collect::<BTreeSet<_>>();

    let mut offset = 0;
    while let Some(relative) = text[offset..].find("export {") {
        let start = offset + relative + "export ".len();
        let Some(close) = find_matching(text, start, '{', '}') else {
            break;
        };
        for specifier in text[start + 1..close].split(',') {
            let exported = specifier
                .split_whitespace()
                .last()
                .unwrap_or_default()
                .trim();
            if is_identifier(exported) {
                names.insert(exported.to_string());
            }
        }
        offset = close + 1;
    }
    names
}

fn role_transaction_allows_both(text: &str) -> bool {
    text.lines().enumerate().any(|(index, line)| {
        parse_top_level_declaration(line, "", index + 1).is_some_and(|declaration| {
            if !declaration.exported
                || declaration.name != "RoleTransaction"
                || declaration.kind != DeclarationKind::Type
            {
                return false;
            }
            let statement = declaration_statement(text, index + 1);
            contains_identifier(&statement, "UserTransaction")
                && contains_identifier(&statement, "ServerTransaction")
        })
    })
}

#[derive(Debug)]
struct ExportedFunction {
    name: String,
    line: usize,
    parameters: String,
}

fn exported_repository_functions(text: &str, file: &str) -> Vec<ExportedFunction> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let declaration = parse_top_level_declaration(line, file, index + 1)?;
            if !declaration.exported
                || !matches!(
                    declaration.kind,
                    DeclarationKind::Function | DeclarationKind::Const
                )
            {
                return None;
            }
            let statement = declaration_statement(text, index + 1);
            if declaration.kind == DeclarationKind::Const
                && !const_initializer_is_function(&statement)
            {
                return None;
            }
            let parameters = function_parameter_sections(&statement)
                .into_iter()
                .next()
                .map(|section| section.text)
                .unwrap_or_default();
            Some(ExportedFunction {
                name: declaration.name,
                line: index + 1,
                parameters,
            })
        })
        .collect()
}

fn const_initializer_is_function(statement: &str) -> bool {
    let Some((_, initializer)) = statement.split_once('=') else {
        return false;
    };
    let initializer = initializer.trim_start();
    if initializer.starts_with("function") || initializer.starts_with("async function") {
        return true;
    }
    initializer.find("=>").is_some_and(|arrow| {
        let prefix = &initializer[..arrow];
        !prefix.contains('{') && !prefix.contains(';')
    })
}

#[derive(Debug)]
struct ParameterSection {
    line: usize,
    text: String,
}

fn function_parameter_sections(text: &str) -> Vec<ParameterSection> {
    let masked = mask_comments_and_strings(text);
    let mut spans = BTreeSet::<(usize, usize)>::new();

    for (index, _) in masked.match_indices("function") {
        if !is_reference_boundary(&masked, index, "function".len()) {
            continue;
        }
        let Some(relative_open) = masked[index + "function".len()..].find('(') else {
            continue;
        };
        let open = index + "function".len() + relative_open;
        if let Some(close) = find_matching(&masked, open, '(', ')') {
            spans.insert((open + 1, close));
        }
    }

    for (arrow, _) in masked.match_indices("=>") {
        let Some((open, close)) = arrow_parameter_bounds(&masked, arrow) else {
            continue;
        };
        spans.insert((open, close));
    }

    spans
        .into_iter()
        .filter_map(|(start, end)| {
            text.get(start..end).map(|parameters| ParameterSection {
                line: line_number(text, start),
                text: parameters.to_string(),
            })
        })
        .collect()
}

fn arrow_parameter_bounds(text: &str, arrow: usize) -> Option<(usize, usize)> {
    let before = &text[..arrow];
    let close = before.rfind(')')?;
    let between = before[close + 1..].trim();
    if !between.is_empty() && !between.starts_with(':') {
        return None;
    }
    let open = find_matching_backward(text, close, '(', ')')?;
    Some((open + 1, close))
}

fn declaration_statement(text: &str, line_number: usize) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let start = line_number.saturating_sub(1);
    let mut statement = String::new();
    let mut brace = 0isize;
    let mut paren = 0isize;
    let mut bracket = 0isize;
    let mut started_body = false;

    for (index, line) in lines.iter().enumerate().skip(start) {
        if index > start
            && brace <= 0
            && paren <= 0
            && bracket <= 0
            && parse_top_level_declaration(line, "", index + 1).is_some()
        {
            break;
        }
        statement.push_str(line);
        statement.push('\n');
        for ch in mask_comments_and_strings(line).chars() {
            match ch {
                '{' => {
                    brace += 1;
                    started_body = true;
                }
                '}' => brace -= 1,
                '(' => paren += 1,
                ')' => paren -= 1,
                '[' => bracket += 1,
                ']' => bracket -= 1,
                _ => {}
            }
        }
        if started_body && brace <= 0 && paren <= 0 && bracket <= 0 {
            break;
        }
        if !started_body && paren <= 0 && bracket <= 0 && line.contains(';') {
            break;
        }
    }
    statement
}

fn declaration_line(text: &str, name: &str) -> Option<usize> {
    text.lines().enumerate().find_map(|(index, line)| {
        parse_top_level_declaration(line, "", index + 1)
            .filter(|declaration| declaration.name == name)
            .map(|_| index + 1)
    })
}

fn normalize_relative(from_file: &str, source: &str) -> String {
    let parent = from_file
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
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
    let path = parts.join("/");
    path.strip_suffix(".tsx")
        .or_else(|| path.strip_suffix(".ts"))
        .or_else(|| path.strip_suffix(".js"))
        .unwrap_or(&path)
        .trim_end_matches("/index")
        .to_string()
}

fn mask_comments_and_strings(text: &str) -> String {
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
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                string = None;
            }
            output.push(if ch == '\n' { '\n' } else { ' ' });
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
        } else if matches!(ch, '\'' | '"' | '`') {
            output.push(' ');
            string = Some(ch);
        } else {
            output.push(ch);
        }
    }
    output
}

fn find_matching(text: &str, open_index: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in text
        .char_indices()
        .skip_while(|(index, _)| *index < open_index)
    {
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

fn find_matching_backward(
    text: &str,
    close_index: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in text[..=close_index].char_indices().rev() {
        if ch == close {
            depth += 1;
        } else if ch == open {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn contains_identifier(text: &str, identifier: &str) -> bool {
    text.match_indices(identifier)
        .any(|(index, _)| is_reference_boundary(text, index, identifier.len()))
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

fn roles_help() -> &'static str {
    "Create src/server/db/roles.ts and directly export withUserRole, withServerRole, UserTransaction, ServerTransaction, and `type RoleTransaction = UserTransaction | ServerTransaction`. Each helper must open an AppDb transaction, apply SET LOCAL ROLE plus request identity/tenant settings with transaction-local semantics, and invoke the caller with only the branded scoped transaction. Exported repository functions then accept RoleTransaction or the narrower user/server type; raw AppDb and ServerContext stay in the orchestration layer."
}

#[cfg(test)]
mod tests {
    use super::{
        const_initializer_is_function, exported_repository_functions, function_parameter_sections,
        import_allowed, role_transaction_allows_both,
    };
    use crate::config::Config;

    #[test]
    fn exported_repository_function_accepts_union_role_transaction() {
        let functions = exported_repository_functions(
            "export async function findUser(tx: RoleTransaction, id: string) { return id; }",
            "src/server/db/repositories/users.repository.ts",
        );
        assert_eq!(functions.len(), 1);
        assert!(functions[0].parameters.contains("RoleTransaction"));
    }

    #[test]
    fn exported_repository_constants_are_not_treated_as_functions() {
        assert!(!const_initializer_is_function(
            "export const userSchema = z.object({ id: z.number() });"
        ));
        assert!(const_initializer_is_function(
            "export const findUser = async (tx: RoleTransaction) => tx;"
        ));
    }

    #[test]
    fn raw_database_types_are_found_only_in_parameter_sections() {
        let sections = function_parameter_sections(
            "import type { AppDb } from '../index';\nconst helper = (tx: AppDb) => tx;",
        );
        assert_eq!(sections.len(), 1);
        assert!(sections[0].text.contains("AppDb"));
    }

    #[test]
    fn role_transaction_must_allow_user_and_server() {
        assert!(role_transaction_allows_both(
            "export type RoleTransaction = UserTransaction | ServerTransaction;"
        ));
        assert!(!role_transaction_allows_both(
            "export type RoleTransaction = UserTransaction;"
        ));
    }

    #[test]
    fn shared_contracts_are_type_only() {
        let config = Config::default();
        let file = "src/server/db/repositories/users.repository.ts";
        let root = "src/server/db";
        assert!(import_allowed(
            "~/shared/contracts/user",
            true,
            file,
            root,
            &config
        ));
        assert!(!import_allowed(
            "~/shared/contracts/user",
            false,
            file,
            root,
            &config
        ));
    }
}
