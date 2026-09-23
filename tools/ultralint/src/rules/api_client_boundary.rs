use std::collections::BTreeSet;

use crate::analysis::CallFact;
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{
    code_only, has_prefix, is_test_file, is_ts_source_file, join, line_number_at,
    matching_delimiter, object_properties, string_literal,
};
use crate::rules::{Report, Rule};

pub struct ApiClientBoundaryRule;

impl Rule for ApiClientBoundaryRule {
    fn id(&self) -> &'static str {
        "api-client-boundary"
    }

    fn category(&self) -> &'static str {
        "api"
    }

    fn description(&self) -> &'static str {
        "requires browser API calls to go through the generated OpenAPI client"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.web_apps {
            if project.exists(&join(app_root, "src/server/api/index.ts")) {
                check_generated_client(project, app_root, report, self.id());
            }
        }

        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) || is_test_file(&file.rel_path) {
                continue;
            }

            for app_root in &config.web_apps {
                check_file(project, file, app_root, report, self.id());
            }
        }
    }
}

fn check_generated_client(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let client_path = join(app_root, "src/lib/api/client.ts");
    let Some(client) = project.read(&client_path) else {
        report.error(
            rule_id,
            client_path,
            None,
            "generated API client wrapper is missing",
            generated_client_help(),
        );
        return;
    };

    for required in [
        "openapi-fetch",
        "openapi-react-query",
        "fetchClient",
        "useApiQuery",
        "useApiMutation",
    ] {
        if client.contains(required) {
            continue;
        }
        report.error(
            rule_id,
            &client_path,
            None,
            format!("generated API client wrapper is missing `{required}`"),
            generated_client_help(),
        );
    }

    if !contains_paths_import(client) {
        report.error(
            rule_id,
            &client_path,
            None,
            "generated API client wrapper is missing `type { paths } from './v1'`",
            generated_client_help(),
        );
    }
}

fn check_file(
    project: &Project,
    file: &crate::fs::ProjectFile,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    if !has_prefix(&file.rel_path, app_root, "src/") {
        return;
    }

    let in_api_client = has_prefix(&file.rel_path, app_root, "src/lib/api/");
    let browser_facing = has_prefix(&file.rel_path, app_root, "src/components/")
        || has_prefix(&file.rel_path, app_root, "src/routes/")
        || has_prefix(&file.rel_path, app_root, "src/lib/");

    if browser_facing && !in_api_client {
        for line_number in raw_api_call_lines(file) {
            report.error(
                rule_id,
                &file.rel_path,
                Some(line_number),
                "raw /api request is not allowed in browser-facing code",
                "Use the generated client from src/lib/api/client.ts instead of fetch('/api...'). Install missing dependencies with `pnpm add openapi-fetch openapi-react-query` and regenerate types with `pnpm add -D openapi-typescript && pnpm gen:api`.",
            );
        }
    }

    if !in_api_client {
        for line_number in handrolled_api_call_lines(file, app_root) {
            report.error(
                rule_id,
                &file.rel_path,
                Some(line_number),
                "hand-rolled API client helper is not allowed",
                "Delete local fetch wrappers such as apiRequest/fetchJson/requestApi and call fetchClient, useApiQuery, or useApiMutation from src/lib/api/client.ts. If the generated client is missing, install with `pnpm add openapi-fetch openapi-react-query` and run `pnpm gen:api`.",
            );
        }
    }

    if !in_api_client
        && is_suspicious_api_client_file(&file.rel_path, app_root)
        && project.exists(&join(app_root, "src/lib/api/client.ts"))
    {
        report.error(
            rule_id,
            &file.rel_path,
            None,
            "API helper file is outside the generated API client boundary",
            "Keep the only browser API client at src/lib/api/client.ts. Move typed path definitions to src/lib/api/v1.d.ts and use openapi-fetch/openapi-react-query from that wrapper.",
        );
    }
}

fn raw_api_call_lines(file: &ProjectFile) -> BTreeSet<usize> {
    let mut lines = file
        .ts
        .as_ref()
        .into_iter()
        .flat_map(|analysis| &analysis.calls)
        .filter(|call| is_raw_api_call(call))
        .map(|call| call.line)
        .collect::<BTreeSet<_>>();
    lines.extend(raw_request_constructor_lines(&file.text));
    lines
}

fn is_raw_api_call(call: &CallFact) -> bool {
    let callee = call.callee.trim().trim_end_matches('?');
    let terminal = callee.rsplit('.').next().unwrap_or(callee);
    if matches!(terminal, "fetch" | "Request") {
        return call
            .arguments
            .first()
            .is_some_and(|argument| has_static_api_target(argument));
    }

    if callee == "axios" {
        return call.arguments.first().is_some_and(|argument| {
            has_static_api_target(argument)
                || object_properties(argument).is_some_and(|properties| {
                    properties.iter().any(|property| {
                        property.name == "url" && has_static_api_target(property.value)
                    })
                })
        });
    }

    let Some((owner, method)) = callee.rsplit_once('.') else {
        return false;
    };
    owner.trim_end_matches('?') == "axios"
        && matches!(
            method,
            "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "request"
        )
        && call.arguments.first().is_some_and(|argument| {
            has_static_api_target(argument)
                || (method == "request"
                    && object_properties(argument).is_some_and(|properties| {
                        properties.iter().any(|property| {
                            property.name == "url" && has_static_api_target(property.value)
                        })
                    }))
        })
}

fn has_static_api_target(argument: &str) -> bool {
    if string_literal(argument).is_some_and(|value| is_api_path(&value)) {
        return true;
    }

    let code = code_only(argument);
    for constructor in ["new Request", "Request", "new URL", "URL"] {
        let Some(marker) = code.find(constructor) else {
            continue;
        };
        let Some(open) = code[marker + constructor.len()..].find('(') else {
            continue;
        };
        let open = marker + constructor.len() + open;
        let Some(close) = matching_delimiter(argument, open) else {
            continue;
        };
        if first_argument(&argument[open + 1..close])
            .as_deref()
            .and_then(string_literal)
            .is_some_and(|value| is_api_path(&value))
        {
            return true;
        }
    }
    false
}

fn first_argument(arguments: &str) -> Option<String> {
    let wrapped = format!("{{ value: {arguments} }}");
    object_properties(&wrapped)?
        .first()
        .map(|property| property.value.to_string())
}

fn raw_request_constructor_lines(text: &str) -> BTreeSet<usize> {
    let code = code_only(text);
    let bytes = code.as_bytes();
    let mut lines = BTreeSet::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if !identifier_at(&code, cursor, "new") {
            cursor += 1;
            continue;
        }
        let mut next = cursor + 3;
        while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
            next += 1;
        }
        if !identifier_at(&code, next, "Request") {
            cursor += 3;
            continue;
        }
        next += "Request".len();
        while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
            next += 1;
        }
        if bytes.get(next) != Some(&b'(') {
            cursor = next;
            continue;
        }
        let Some(close) = matching_delimiter(text, next) else {
            cursor = next + 1;
            continue;
        };
        if first_argument(&text[next + 1..close])
            .as_deref()
            .and_then(string_literal)
            .is_some_and(|value| is_api_path(&value))
        {
            lines.insert(line_number_at(text, cursor));
        }
        cursor = close + 1;
    }
    lines
}

fn identifier_at(text: &str, offset: usize, identifier: &str) -> bool {
    if text.as_bytes().get(offset..offset + identifier.len()) != Some(identifier.as_bytes()) {
        return false;
    }
    let before = offset
        .checked_sub(1)
        .and_then(|index| text.as_bytes().get(index));
    let after = text.as_bytes().get(offset + identifier.len());
    !before.is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        && !after.is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

fn is_api_path(value: &str) -> bool {
    value == "/api"
        || value.starts_with("/api/")
        || value.starts_with("/api?")
        || value.starts_with("/api#")
}

fn contains_paths_import(text: &str) -> bool {
    text.contains("type { paths } from './v1'") || text.contains("type { paths } from \"./v1\"")
}

fn handrolled_api_call_lines(file: &ProjectFile, app_root: &str) -> BTreeSet<usize> {
    if !has_prefix(&file.rel_path, app_root, "src/lib/")
        && !has_prefix(&file.rel_path, app_root, "src/components/")
    {
        return BTreeSet::new();
    }
    file.ts
        .as_ref()
        .into_iter()
        .flat_map(|analysis| &analysis.calls)
        .filter(|call| {
            let terminal = call.callee.rsplit('.').next().unwrap_or(&call.callee);
            matches!(
                terminal,
                "apiRequest" | "authApiRequest" | "fetchJson" | "requestApi"
            )
        })
        .map(|call| call.line)
        .collect()
}

fn is_suspicious_api_client_file(rel_path: &str, app_root: &str) -> bool {
    has_prefix(rel_path, app_root, "src/lib/")
        && (rel_path.contains("Api") || rel_path.contains("api"))
        && !rel_path.ends_with("src/lib/api/client.ts")
        && !rel_path.ends_with("src/lib/api/v1.d.ts")
}

fn generated_client_help() -> &'static str {
    "Create src/lib/api/client.ts with the generated client boundary. Install dependencies with `pnpm add openapi-fetch openapi-react-query` and `pnpm add -D openapi-typescript`, then run `pnpm gen:api`. If src/lib/api/v1.d.ts contains paths like `/api/v1/...`, use `baseUrl: ''`; only use `baseUrl: '/api/v1'` when generated paths are prefix-free.\n\nimport createFetchClient from 'openapi-fetch';\nimport createClient from 'openapi-react-query';\nimport type { paths } from './v1';\n\nexport const fetchClient = createFetchClient<paths>({ baseUrl: '' });\nexport const { useQuery: useApiQuery, useMutation: useApiMutation } = createClient(fetchClient);"
}

#[cfg(test)]
mod tests {
    use crate::analysis::analyze_typescript;

    use super::{is_raw_api_call, raw_request_constructor_lines};

    #[test]
    fn catches_multiline_fetch_and_axios_calls_from_oxc_facts() {
        let analysis = analyze_typescript(
            "src/lib/example.ts",
            r#"
fetch(
  '/api/v1/users',
);
axios.post(
  "/api/v1/users",
  payload,
);
"#,
        );

        assert_eq!(
            analysis
                .calls
                .iter()
                .filter(|call| is_raw_api_call(call))
                .count(),
            2
        );
    }

    #[test]
    fn ignores_api_text_in_comments_and_strings_but_finds_new_request() {
        let source = r#"
// fetch('/api/v1/comment')
const example = "axios.get('/api/v1/string')";
const request = new Request(
  '/api/v1/real',
);
"#;
        assert_eq!(raw_request_constructor_lines(source), [4].into());
    }
}
