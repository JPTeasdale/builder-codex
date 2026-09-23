use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::ImportKind;
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::is_ts_source_file;
use crate::rules::{Report, Rule};

pub struct OriginPolicySafetyRule;

impl Rule for OriginPolicySafetyRule {
    fn id(&self) -> &'static str {
        "origin-policy-safety"
    }

    fn category(&self) -> &'static str {
        "security"
    }

    fn description(&self) -> &'static str {
        "rejects wildcard Better Auth trust and credentialed wildcard CORS while preserving intentional credential-free public APIs"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) || file.generated {
                continue;
            }
            check_better_auth_origins(file, report, self.id());
            check_hono_cors(file, report, self.id());
            check_header_policies(file, report, self.id());
        }
    }
}

fn check_better_auth_origins(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let bindings = imported_bindings(file, "better-auth", "betterAuth");
    let uses_better_auth = file.ts.as_ref().is_some_and(|analysis| {
        analysis
            .calls
            .iter()
            .any(|call| bindings.contains(&call.callee))
    });
    if !uses_better_auth {
        return;
    }

    if let Some(index) = object_with_property_values(&file.text)
        .into_iter()
        .find_map(|(index, properties)| {
            properties
                .get("trustedOrigins")
                .is_some_and(|value| contains_wildcard_string(value))
                .then_some(index)
        })
    {
        report.error(
            rule_id,
            &file.rel_path,
            Some(line_number(&file.text, index)),
            "Better Auth trustedOrigins contains a wildcard",
            "List exact origins per environment, typically `[env.APP_URL]`, and validate APP_URL during startup. Do not use `*`, wildcard subdomains, or pattern strings for Better Auth trust: auth endpoints set/read credentials and must have an explicit origin allowlist. Give preview environments their exact preview URL rather than broadening production trust.",
        );
    }
}

fn check_hono_cors(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let bindings = imported_bindings(file, "hono/cors", "cors");
    let Some(analysis) = file.ts.as_ref() else {
        return;
    };
    for call in analysis
        .calls
        .iter()
        .filter(|call| bindings.contains(&call.callee))
    {
        let properties = call
            .arguments
            .first()
            .map(|argument| object_properties(argument))
            .unwrap_or_default();
        let wildcard = properties
            .get("origin")
            .is_some_and(|value| contains_wildcard_string(value));
        let credentialed = properties
            .get("credentials")
            .is_some_and(|value| is_true(value));
        if wildcard && credentialed {
            report.error(
                rule_id,
                &file.rel_path,
                Some(call.line),
                "Hono CORS enables credentials with a wildcard origin",
                cors_help(),
            );
        }
    }
}

fn check_header_policies(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let Some(analysis) = file.ts.as_ref() else {
        return;
    };
    let mut header_receivers = BTreeMap::<String, HeaderPolicy>::new();
    for call in &analysis.calls {
        if !(call.callee.ends_with(".header") || call.callee.ends_with(".set")) {
            continue;
        }
        let Some(name) = call.arguments.first().map(|value| unquote(value.trim())) else {
            continue;
        };
        let value = call.arguments.get(1).map(String::as_str).unwrap_or("");
        let receiver = call
            .callee
            .rsplit_once('.')
            .map(|(receiver, _)| receiver)
            .unwrap_or(&call.callee)
            .to_string();
        let policy = header_receivers.entry(receiver).or_default();
        if name.eq_ignore_ascii_case("Access-Control-Allow-Origin")
            && contains_wildcard_string(value)
        {
            policy.wildcard_line = Some(call.line);
        }
        if name.eq_ignore_ascii_case("Access-Control-Allow-Credentials") && is_true(value) {
            policy.credentials = true;
        }
    }

    for policy in header_receivers.values() {
        if policy.credentials
            && let Some(line) = policy.wildcard_line
        {
            report.error(
                rule_id,
                &file.rel_path,
                Some(line),
                "response headers combine wildcard CORS origin with credentials",
                cors_help(),
            );
        }
    }

    if let Some(index) = object_with_property_values(&file.text)
        .into_iter()
        .find_map(|(index, properties)| {
            let wildcard = properties
                .get("Access-Control-Allow-Origin")
                .is_some_and(|value| contains_wildcard_string(value));
            let credentials = properties
                .get("Access-Control-Allow-Credentials")
                .is_some_and(|value| is_true(value));
            (wildcard && credentials).then_some(index)
        })
    {
        report.error(
            rule_id,
            &file.rel_path,
            Some(line_number(&file.text, index)),
            "header object combines wildcard CORS origin with credentials",
            cors_help(),
        );
    }
}

#[derive(Debug, Default)]
struct HeaderPolicy {
    wildcard_line: Option<usize>,
    credentials: bool,
}

fn imported_bindings(file: &ProjectFile, source: &str, exported: &str) -> BTreeSet<String> {
    let Some(analysis) = file.ts.as_ref() else {
        return BTreeSet::new();
    };
    if !analysis.imports.iter().any(|import| {
        import.source == source && import.kind == ImportKind::Static && !import.type_only
    }) {
        return BTreeSet::new();
    }

    let mut bindings = BTreeSet::new();
    for quote in ['\'', '"'] {
        let needle = format!("{quote}{source}{quote}");
        for (source_index, _) in file.text.match_indices(&needle) {
            let before = &file.text[..source_index];
            let Some(import_index) = before.rfind("import") else {
                continue;
            };
            let head = &before[import_index + "import".len()..];
            if head.contains(';')
                || !head.contains("from")
                || head.trim_start().starts_with("type ")
            {
                continue;
            }
            let (Some(open), Some(close)) = (head.find('{'), head.rfind('}')) else {
                continue;
            };
            for specifier in head[open + 1..close].split(',') {
                let words = specifier.split_whitespace().collect::<Vec<_>>();
                if words.first().copied() != Some(exported) {
                    continue;
                }
                let local = if words.get(1).copied() == Some("as") {
                    words.get(2).copied().unwrap_or(exported)
                } else {
                    exported
                };
                if is_identifier(local) {
                    bindings.insert(local.to_string());
                }
            }
        }
    }
    bindings
}

fn object_with_property_values(text: &str) -> Vec<(usize, BTreeMap<String, String>)> {
    let mut objects = Vec::new();
    for (index, ch) in text.char_indices() {
        if ch != '{' {
            continue;
        }
        let Some(close) = find_matching(text, index, '{', '}') else {
            continue;
        };
        let properties = object_properties(&text[index..=close]);
        if !properties.is_empty() {
            objects.push((index, properties));
        }
    }
    objects
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

fn contains_wildcard_string(value: &str) -> bool {
    let mut string: Option<char> = None;
    let mut escaped = false;
    let mut content = String::new();
    for ch in value.chars() {
        if let Some(quote) = string {
            if escaped {
                escaped = false;
                content.push(ch);
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                if content.contains('*') {
                    return true;
                }
                content.clear();
                string = None;
            } else {
                content.push(ch);
            }
        } else if matches!(ch, '\'' | '"' | '`') {
            string = Some(ch);
        }
    }
    false
}

fn is_true(value: &str) -> bool {
    unquote(value.trim()).eq_ignore_ascii_case("true")
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

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_' || ch == '$')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

fn line_number(text: &str, byte_index: usize) -> usize {
    text[..byte_index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn cors_help() -> &'static str {
    "Credentialed CORS must return an exact validated request origin and include `Vary: Origin`; never combine credentials with `*`. Configure Hono cors with an explicit allowlist/origin callback and `credentials: true`, or remove credentials for a genuinely public API. A wildcard origin by itself remains valid only for endpoints that do not use cookies, authorization credentials, or Access-Control-Allow-Credentials."
}

#[cfg(test)]
mod tests {
    use super::{contains_wildcard_string, is_true, object_properties};

    #[test]
    fn wildcard_strings_include_subdomain_patterns() {
        assert!(contains_wildcard_string("['*']"));
        assert!(contains_wildcard_string("['https://*.example.com']"));
        assert!(!contains_wildcard_string("['https://app.example.com']"));
    }

    #[test]
    fn credentialed_wildcard_cors_is_distinguishable_from_public_cors() {
        let credentialed = object_properties("{ origin: '*', credentials: true }");
        assert!(contains_wildcard_string(&credentialed["origin"]));
        assert!(is_true(&credentialed["credentials"]));

        let public = object_properties("{ origin: '*' }");
        assert!(contains_wildcard_string(&public["origin"]));
        assert!(!public.contains_key("credentials"));
    }
}
