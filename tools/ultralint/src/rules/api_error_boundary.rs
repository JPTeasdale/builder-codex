use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{code_only, has_prefix, is_test_file, is_ts_source_file, join};
use crate::rules::{Report, Rule};

pub struct ApiErrorBoundaryRule;

impl Rule for ApiErrorBoundaryRule {
    fn id(&self) -> &'static str {
        "api-error-boundary"
    }

    fn category(&self) -> &'static str {
        "api"
    }

    fn description(&self) -> &'static str {
        "requires a centralized Hono error boundary and prevents raw error disclosure"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for app_root in &config.web_apps {
            let api_index = join(app_root, "src/server/api/index.ts");
            if !project.is_file(&api_index) {
                continue;
            }

            let has_error_boundary = project
                .file(&api_index)
                .and_then(|file| file.ts.as_ref())
                .is_some_and(|analysis| {
                    analysis
                        .calls
                        .iter()
                        .any(|call| terminal_call_name(&call.callee) == "onError")
                });
            if !has_error_boundary {
                report.error(
                    self.id(),
                    &api_index,
                    None,
                    "mounted Hono API has no centralized onError boundary",
                    error_boundary_help(),
                );
            }

            for file in project.files.iter().filter(|file| {
                is_ts_source_file(&file.rel_path)
                    && !is_test_file(&file.rel_path)
                    && has_prefix(&file.rel_path, app_root, "src/server/api/")
            }) {
                check_error_responses(file, report, self.id());
            }
        }
    }
}

fn check_error_responses(file: &ProjectFile, report: &mut Report, rule_id: &'static str) {
    let Some(analysis) = &file.ts else {
        return;
    };
    for call in &analysis.calls {
        if !is_response_call(&call.callee) {
            continue;
        }
        let Some(body) = call.arguments.first() else {
            continue;
        };
        if !exposes_raw_error(body) {
            continue;
        }
        report.error(
            rule_id,
            &file.rel_path,
            Some(call.line),
            "API response exposes a raw error object or implementation detail",
            "Return a stable public error shape and log the original exception only on the server. For example: `api.onError((error, c) => { console.error(error); return c.json({ error: { code: \"INTERNAL_ERROR\", message: \"Unexpected error\" } }, 500); });`. Do not return `error`, `error.message`, `error.stack`, `error.cause`, `String(error)`, or `JSON.stringify(error)`. Install Hono with `pnpm add hono` if the API dependency is missing, then run `pnpm test` and `pnpm gen:api`.",
        );
    }
}

fn is_response_call(callee: &str) -> bool {
    matches!(
        terminal_call_name(callee),
        "json" | "text" | "html" | "body" | "newResponse"
    )
}

fn terminal_call_name(callee: &str) -> &str {
    callee
        .trim()
        .trim_end_matches('?')
        .rsplit('.')
        .next()
        .unwrap_or(callee)
        .trim_end_matches('?')
}

fn exposes_raw_error(body: &str) -> bool {
    let code = code_only(body).to_ascii_lowercase();
    [".message", ".stack", ".cause"]
        .iter()
        .any(|part| code.contains(part))
        || ["error", "err", "exception"]
            .iter()
            .any(|identifier| contains_identifier(&code, identifier))
}

fn contains_identifier(text: &str, identifier: &str) -> bool {
    text.match_indices(identifier).any(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + identifier.len()..].chars().next();
        let property_key = text[index + identifier.len()..]
            .trim_start()
            .starts_with(':');
        !property_key
            && before.is_none_or(|ch| !is_identifier_char(ch))
            && after.is_none_or(|ch| !is_identifier_char(ch))
    })
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')
}

fn error_boundary_help() -> &'static str {
    "Register one error boundary where the Hono OpenAPI app is created. For example: `api.onError((error, c) => { console.error(error); return c.json({ error: { code: \"INTERNAL_ERROR\", message: \"Unexpected error\" } }, 500); });`. Log request IDs and the original exception server-side, but return only stable public codes/messages. Install Hono with `pnpm add hono` if needed, cover the 500 shape with `pnpm test`, and refresh the generated contract with `pnpm gen:api`."
}

#[cfg(test)]
mod tests {
    use super::exposes_raw_error;

    #[test]
    fn raw_errors_are_distinct_from_safe_literal_messages() {
        assert!(exposes_raw_error("{ error: err.message }"));
        assert!(exposes_raw_error("JSON.stringify(error)"));
        assert!(!exposes_raw_error(
            "{ error: { code: 'INTERNAL_ERROR', message: 'Unexpected error' } }"
        ));
    }
}
