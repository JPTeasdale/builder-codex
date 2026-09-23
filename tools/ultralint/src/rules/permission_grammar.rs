use std::collections::BTreeSet;

use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{is_test_file, is_ts_source_file};
use crate::rules::{Report, Rule};

pub struct PermissionGrammarRule;

impl Rule for PermissionGrammarRule {
    fn id(&self) -> &'static str {
        "permission-grammar"
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "derives permission resources from Drizzle tables and centralizes role semantics"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        let resources = table_resources(project);
        if resources.is_empty() {
            return;
        }

        for file in &project.files {
            if file.generated || !is_ts_source_file(&file.rel_path) {
                continue;
            }
            check_permission_strings(file, &resources, report, self.id());
            if !is_test_file(&file.rel_path) && !is_rbac_owner(&file.rel_path) {
                check_direct_role_checks(file, report, self.id());
            }
        }
    }
}

fn table_resources(project: &Project) -> BTreeSet<String> {
    let mut resources = BTreeSet::new();
    for file in &project.files {
        let Some(analysis) = &file.ts else {
            continue;
        };
        for call in &analysis.calls {
            if terminal_name(&call.callee) != "pgTable" {
                continue;
            }
            let Some(table) = call
                .arguments
                .first()
                .and_then(|value| unquote(value.trim()))
            else {
                continue;
            };
            if table.starts_with('_') || !is_permission_token(table) {
                continue;
            }
            resources.insert(table.to_string());
            if let Some(resource) = table.rsplit('_').next()
                && resource != table
                && is_permission_token(resource)
            {
                resources.insert(resource.to_string());
            }
        }
    }
    resources
}

fn check_permission_strings(
    file: &ProjectFile,
    resources: &BTreeSet<String>,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(analysis) = &file.ts else {
        return;
    };
    let mut candidates = BTreeSet::<(usize, String)>::new();
    let permission_owner = is_rbac_owner(&file.rel_path);

    for string in &analysis.strings {
        if !looks_like_permission_prefix(&string.value) {
            continue;
        }
        let line = source_line(&file.text, string.line).to_ascii_lowercase();
        if permission_owner || line_mentions_permission(&line) {
            candidates.insert((string.line, string.value.clone()));
        }
    }

    for call in &analysis.calls {
        if !is_permission_call(&call.callee) {
            continue;
        }
        for argument in &call.arguments {
            if let Some(value) = unquote(argument.trim())
                && looks_like_permission_prefix(value)
            {
                candidates.insert((call.line, value.to_string()));
            }
        }
    }

    for (line, permission) in candidates {
        let segments = permission.split(':').collect::<Vec<_>>();
        if segments.len() < 3 || segments.iter().any(|segment| !is_permission_token(segment)) {
            report.error(
                rule_id,
                &file.rel_path,
                Some(line),
                format!("permission `{permission}` does not use the scoped permission grammar"),
                "Use `<resource>:<action>:<scope>` (and optional narrower scope segments), with lowercase kebab/snake tokens. For example: `documents:read:submitted:region`. Define permission constants and evaluation in one RBAC module under src/server/auth or src/server/domain, then run `pnpm ultralint`.",
            );
            continue;
        }
        if resources.contains(segments[0]) {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            Some(line),
            format!(
                "permission `{permission}` uses unknown resource `{}`",
                segments[0]
            ),
            format!(
                "Use a resource generated from a non-underscore Drizzle pgTable name. Available resources: {}. Keep aliases tied to the final underscore-delimited table segment, define permissions centrally, and run `pnpm ultralint`.",
                resources.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        );
    }
}

fn check_direct_role_checks(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let Some(analysis) = &file.ts else {
        return;
    };
    for string in &analysis.strings {
        if !looks_like_role_name(&string.value) {
            continue;
        }
        let line = source_line(&file.text, string.line);
        if !line_uses_role_check(line) {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            Some(string.line),
            format!(
                "direct role check against `{}` bypasses centralized permissions",
                string.value
            ),
            "Move role-to-permission mapping into one RBAC module under src/server/auth or src/server/domain. Call a named permission helper such as `hasPermission(actor, \"documents:read:submitted:region\")` instead of comparing roles in routes, repositories, services, or components. This keeps regional/global scope enforceable and reviewable.",
        );
    }
}

fn is_rbac_owner(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.contains("/rbac/")
        || path.contains("/rbac.")
        || path.contains("permission")
        || path.contains("authorization")
        || path.contains("access-control")
        || path.ends_with("/roles.ts")
        || path.contains("/roles/")
}

fn line_mentions_permission(line: &str) -> bool {
    [
        "permission",
        "authorize",
        "hasaccess",
        "has_access",
        "can(",
        ".can(",
    ]
    .iter()
    .any(|marker| line.contains(marker))
}

fn is_permission_call(callee: &str) -> bool {
    let terminal = terminal_name(callee).to_ascii_lowercase();
    terminal.contains("permission")
        || terminal.contains("authorize")
        || terminal.contains("access")
        || matches!(
            terminal.as_str(),
            "can" | "cannot" | "allowed" | "isallowed"
        )
}

fn looks_like_permission_prefix(value: &str) -> bool {
    if value.contains("://") {
        return false;
    }
    let mut segments = value.split(':');
    segments.next().is_some_and(is_permission_token)
        && segments.next().is_some_and(is_permission_token)
}

fn is_permission_token(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_lowercase())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
}

fn looks_like_role_name(value: &str) -> bool {
    value == "admin"
        || value == "superadmin"
        || value.ends_with("-admin")
        || value.ends_with("_admin")
        || value.starts_with("admin-")
        || value.starts_with("admin_")
}

fn line_uses_role_check(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains("===")
        || line.contains("!==")
        || lower.contains("role.includes(")
        || lower.contains("roles.includes(")
        || lower.contains("hasrole(")
        || lower.contains("isrole(")
        || lower.contains("requirerole(")
}

fn terminal_name(callee: &str) -> &str {
    callee.rsplit('.').next().unwrap_or(callee)
}

fn source_line(text: &str, line: usize) -> &str {
    text.lines().nth(line.saturating_sub(1)).unwrap_or_default()
}

fn unquote(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
}

#[cfg(test)]
mod tests {
    use super::{is_permission_token, looks_like_permission_prefix, looks_like_role_name};

    #[test]
    fn recognizes_scoped_permission_and_role_shapes() {
        assert!(looks_like_permission_prefix(
            "documents:read:submitted:region"
        ));
        assert!(!looks_like_permission_prefix("https://example.com"));
        assert!(is_permission_token("application_documents"));
        assert!(!is_permission_token("ApplicationDocuments"));
        assert!(looks_like_role_name("region-admin"));
    }
}
