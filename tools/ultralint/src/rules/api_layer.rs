use std::collections::{BTreeMap, BTreeSet};

use crate::analysis::{CallFact, TsAnalysis};
use crate::config::Config;
use crate::fs::{Project, ProjectFile};
use crate::rules::common::{
    code_only, has_prefix, is_test_file, is_ts_source_file, matching_delimiter, object_properties,
    string_literal,
};
use crate::rules::{Report, Rule};

pub struct ApiLayerRule;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RouteContract {
    pub app_root: String,
    pub file: String,
    pub line: usize,
    pub symbol: Option<String>,
    pub method: Option<String>,
    pub path: Option<String>,
    pub operation_id: Option<String>,
    response_statuses: Option<BTreeSet<u16>>,
    request: RequestContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestContract {
    kinds: Option<BTreeSet<String>>,
    unresolved_body: bool,
    params: ParamsContract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParamsContract {
    Missing,
    Unresolved,
    Resolved(BTreeSet<String>),
}

#[derive(Debug, Clone)]
struct RouteMount {
    file: String,
    line: usize,
    symbol: String,
    handler: String,
}

impl Rule for ApiLayerRule {
    fn id(&self) -> &'static str {
        "api-layer"
    }

    fn category(&self) -> &'static str {
        "api"
    }

    fn description(&self) -> &'static str {
        "enforces Hono OpenAPI route modules and versioned API paths"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        let mut routes = Vec::new();
        let mut mounts = Vec::new();

        for app_root in &config.web_apps {
            routes.extend(collect_route_contracts(project, app_root));
            mounts.extend(collect_route_mounts(project, app_root));
            check_api_files(project, app_root, report, self.id());
        }

        for route in &routes {
            check_route_definition(project, route, report, self.id());
        }
        check_operation_ids(&routes, report, self.id());
        check_mounts(project, &routes, &mounts, report, self.id());
    }
}

pub(super) fn collect_route_contracts(project: &Project, app_root: &str) -> Vec<RouteContract> {
    project
        .files
        .iter()
        .filter(|file| {
            is_ts_source_file(&file.rel_path)
                && !is_test_file(&file.rel_path)
                && has_prefix(&file.rel_path, app_root, "src/server/api/")
        })
        .flat_map(|file| {
            file.ts
                .as_ref()
                .into_iter()
                .flat_map(|analysis| &analysis.calls)
                .filter(|call| is_create_route_call(call))
                .filter_map(|call| parse_route_contract(file, app_root, call))
        })
        .collect()
}

fn parse_route_contract(
    file: &ProjectFile,
    app_root: &str,
    call: &CallFact,
) -> Option<RouteContract> {
    let object = call.arguments.first()?;
    let properties = object_properties(object)?;
    let property = |name: &str| {
        properties
            .iter()
            .find(|property| property.name == name)
            .map(|property| property.value)
    };

    let method = property("method")
        .and_then(string_literal)
        .map(|method| method.to_ascii_lowercase());
    let path = property("path").and_then(string_literal);
    let operation_id = property("operationId").and_then(string_literal);
    let response_statuses = property("responses").and_then(parse_response_statuses);
    let request = parse_request_contract(property("request"));

    Some(RouteContract {
        app_root: app_root.to_string(),
        file: file.rel_path.clone(),
        line: call.line,
        symbol: route_symbol(file, call),
        method,
        path,
        operation_id,
        response_statuses,
        request,
    })
}

fn parse_response_statuses(value: &str) -> Option<BTreeSet<u16>> {
    Some(
        object_properties(value)?
            .into_iter()
            .filter_map(|property| property.name.parse::<u16>().ok())
            .collect(),
    )
}

fn parse_request_contract(request: Option<&str>) -> RequestContract {
    let Some(request) = request else {
        return RequestContract {
            kinds: Some(BTreeSet::new()),
            unresolved_body: false,
            params: ParamsContract::Missing,
        };
    };
    let Some(properties) = object_properties(request) else {
        return RequestContract {
            kinds: None,
            unresolved_body: true,
            params: ParamsContract::Unresolved,
        };
    };

    let mut kinds = BTreeSet::new();
    for (request_key, valid_kind) in [
        ("query", "query"),
        ("params", "param"),
        ("headers", "header"),
        ("cookies", "cookie"),
        ("json", "json"),
        ("form", "form"),
    ] {
        if properties
            .iter()
            .any(|property| property.name == request_key)
        {
            kinds.insert(valid_kind.to_string());
        }
    }

    let body = properties
        .iter()
        .find(|property| property.name == "body")
        .map(|property| property.value);
    let unresolved_body = body.is_some_and(|body| !collect_body_kinds(body, &mut kinds));
    let params = properties
        .iter()
        .find(|property| property.name == "params")
        .map_or(ParamsContract::Missing, |property| {
            inline_zod_object_keys(property.value)
                .map(ParamsContract::Resolved)
                .unwrap_or(ParamsContract::Unresolved)
        });

    RequestContract {
        kinds: Some(kinds),
        unresolved_body,
        params,
    }
}

fn collect_body_kinds(body: &str, kinds: &mut BTreeSet<String>) -> bool {
    let Some(body_properties) = object_properties(body) else {
        return false;
    };
    let Some(content) = body_properties
        .iter()
        .find(|property| property.name == "content")
    else {
        return true;
    };
    let Some(content_types) = object_properties(content.value) else {
        return false;
    };
    for content_type in content_types {
        match content_type.name.as_str() {
            "application/json" | "application/*+json" => {
                kinds.insert("json".to_string());
            }
            "application/x-www-form-urlencoded" | "multipart/form-data" => {
                kinds.insert("form".to_string());
            }
            _ => {}
        }
    }
    true
}

fn inline_zod_object_keys(value: &str) -> Option<BTreeSet<String>> {
    let code = code_only(value);
    let marker = code.find("z.object").or_else(|| code.find("object"))?;
    let open = code[marker..].find('(')? + marker;
    let close = matching_delimiter(value, open)?;
    let argument = value[open + 1..close].trim();
    Some(
        object_properties(argument)?
            .into_iter()
            .map(|property| property.name)
            .collect(),
    )
}

fn route_symbol(file: &ProjectFile, call: &CallFact) -> Option<String> {
    let line_start = line_start_offset(&file.text, call.line)?;
    let call_offset = file.text[line_start..].find(&call.text)? + line_start;
    let prefix = &file.text[..call_offset];
    let declaration = ["const ", "let ", "var "]
        .into_iter()
        .filter_map(|keyword| prefix.rfind(keyword).map(|offset| (offset, keyword.len())))
        .max_by_key(|(offset, _)| *offset)?;
    let declaration_text = &prefix[declaration.0 + declaration.1..];
    if declaration_text.contains(';') {
        return None;
    }
    let name = declaration_text
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$'))
        .collect::<String>();
    if name.is_empty() {
        return None;
    }
    let assignment = declaration_text.rfind('=')?;
    if declaration_text[assignment + 1..].trim().is_empty() {
        Some(name)
    } else {
        None
    }
}

fn line_start_offset(text: &str, line: usize) -> Option<usize> {
    if line == 1 {
        return Some(0);
    }
    text.match_indices('\n')
        .nth(line.saturating_sub(2))
        .map(|(offset, _)| offset + 1)
}

fn check_route_definition(
    project: &Project,
    route: &RouteContract,
    report: &mut Report,
    rule_id: &'static str,
) {
    if route.method.is_none() {
        report.error(
            rule_id,
            &route.file,
            Some(route.line),
            route_label(route, "createRoute has no literal method"),
            "Give this createRoute object its own literal method such as `method: 'get'`; shared spreads and dynamic values cannot produce a reliable OpenAPI contract. Verify with `pnpm gen:api`.",
        );
    }
    if route.path.is_none() {
        report.error(
            rule_id,
            &route.file,
            Some(route.line),
            route_label(route, "createRoute has no literal path"),
            "Give this createRoute object its own literal `/api/v1/...` path so route mounting and generated contracts can be checked, then run `pnpm gen:api`.",
        );
    }
    if route.response_statuses.is_none() {
        report.error(
            rule_id,
            &route.file,
            Some(route.line),
            route_label(route, "createRoute is missing its own responses object"),
            "Define a `responses: { ... }` object directly on this createRoute call, including schemas for every JSON status. Keep streaming or binary responses explicitly documented; a narrow `api-layer` suppression may be placed immediately above only the exceptional route when static validation is impossible. Run `pnpm gen:api` afterward.",
        );
    }

    if let Some(path) = &route.path {
        if !is_allowed_api_path(path) {
            report.error(
                rule_id,
                &route.file,
                Some(route.line),
                format!("API route path `{path}` is not versioned"),
                "Use /api/v1/... for app endpoints. Allowed exceptions are /api/auth/*, /api/v1/openapi.json, and /api/health when intentionally unversioned. Regenerate with `pnpm gen:api`.",
            );
        }
        if path.contains("/test/") && !route_is_test_gated(project, route) {
            report.error(
                rule_id,
                &route.file,
                Some(route.line),
                "test API route is not gated to the test environment",
                "Guard test-only registration with `APP_ENV === 'test'` or `isAuthTestEnvironment(env)`, then exercise it with `pnpm test`.",
            );
        }
        check_path_params(route, path, report, rule_id);
    }
}

fn check_path_params(
    route: &RouteContract,
    path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let placeholders = path_placeholders(path);
    match &route.request.params {
        ParamsContract::Missing if !placeholders.is_empty() => report.error(
            rule_id,
            &route.file,
            Some(route.line),
            format!(
                "{} has path placeholders but no request.params schema",
                route_name(route)
            ),
            "Add `request: { params: z.object({ ... }) }` with one field for every `{placeholder}`, read it with `c.req.valid('param')`, and run `pnpm gen:api`.",
        ),
        ParamsContract::Resolved(params) => {
            let missing = placeholders.difference(params).cloned().collect::<Vec<_>>();
            let extra = params.difference(&placeholders).cloned().collect::<Vec<_>>();
            if !missing.is_empty() || !extra.is_empty() {
                report.error(
                    rule_id,
                    &route.file,
                    Some(route.line),
                    format!(
                        "{} path placeholders and request.params differ (missing: {}; extra: {})",
                        route_name(route),
                        display_names(&missing),
                        display_names(&extra)
                    ),
                    "Make the literal `{placeholder}` names and inline request.params z.object fields match exactly, then run `pnpm gen:api`.",
                );
            }
        }
        ParamsContract::Missing | ParamsContract::Unresolved => {}
    }
}

fn check_operation_ids(routes: &[RouteContract], report: &mut Report, rule_id: &'static str) {
    let mut by_id: BTreeMap<&str, Vec<&RouteContract>> = BTreeMap::new();
    for route in routes {
        if let Some(operation_id) = &route.operation_id {
            by_id.entry(operation_id).or_default().push(route);
        }
    }
    for (operation_id, duplicates) in by_id {
        if duplicates.len() < 2 {
            continue;
        }
        for route in duplicates {
            report.error(
                rule_id,
                &route.file,
                Some(route.line),
                format!("operationId `{operation_id}` is not unique within the project"),
                "Assign a stable, project-unique literal operationId to each conflicting createRoute object and run `pnpm gen:api`.",
            );
        }
    }
}

fn collect_route_mounts(project: &Project, app_root: &str) -> Vec<RouteMount> {
    project
        .files
        .iter()
        .filter(|file| {
            is_ts_source_file(&file.rel_path)
                && !is_test_file(&file.rel_path)
                && has_prefix(&file.rel_path, app_root, "src/server/api/")
        })
        .flat_map(|file| {
            file.ts
                .as_ref()
                .into_iter()
                .flat_map(|analysis| &analysis.calls)
                .filter(|call| is_openapi_mount(call))
                .filter_map(|call| {
                    Some(RouteMount {
                        file: file.rel_path.clone(),
                        line: call.line,
                        symbol: referenced_symbol(call.arguments.first()?)?,
                        handler: call.arguments.get(1).cloned().unwrap_or_default(),
                    })
                })
        })
        .collect()
}

fn check_mounts(
    project: &Project,
    routes: &[RouteContract],
    mounts: &[RouteMount],
    report: &mut Report,
    rule_id: &'static str,
) {
    for route in routes {
        let Some(symbol) = &route.symbol else {
            continue;
        };
        let matching = mounts
            .iter()
            .filter(|mount| mount.symbol == *symbol && mount_in_app(mount, &route.app_root))
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            report.error(
                rule_id,
                &route.file,
                Some(route.line),
                format!(
                    "route symbol `{symbol}` is mounted {} times; expected exactly once",
                    matching.len()
                ),
                "Mount each exported route symbol exactly once with `app.openapi(route, handler)` under src/server/api, then run `pnpm gen:api`.",
            );
            continue;
        }
        check_handler_contract(project, route, matching[0], report, rule_id);
    }
}

fn check_handler_contract(
    project: &Project,
    route: &RouteContract,
    mount: &RouteMount,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(file) = project.file(&mount.file) else {
        return;
    };
    let Some(analysis) = &file.ts else {
        return;
    };
    for call in calls_within_handler(analysis, mount) {
        if call.callee.ends_with(".req.valid") {
            let Some(kind) = call
                .arguments
                .first()
                .and_then(|value| string_literal(value))
            else {
                continue;
            };
            if route.request.allows(&kind) == Some(false) {
                report.error(
                    rule_id,
                    &mount.file,
                    Some(call.line),
                    format!(
                        "{} handler validates `{kind}` without a matching request schema",
                        route_name(route)
                    ),
                    "Add the matching request schema to this createRoute object (`body`, `query`, `params`, `headers`, or `cookies`) or remove the c.req.valid call, then run `pnpm gen:api`.",
                );
            }
        }

        if call.callee.ends_with(".json") && call.callee != "Response.json" {
            let status = call
                .arguments
                .get(1)
                .and_then(|value| value.trim().parse::<u16>().ok())
                .or_else(|| (call.arguments.len() == 1).then_some(200));
            let (Some(status), Some(declared)) = (status, &route.response_statuses) else {
                continue;
            };
            if !declared.contains(&status) {
                report.error(
                    rule_id,
                    &mount.file,
                    Some(call.line),
                    format!(
                        "{} handler returns literal JSON status {status} without declaring it",
                        route_name(route)
                    ),
                    "Add this status to the route's responses object with its JSON schema, or return a declared literal status, then run `pnpm gen:api`. Streaming and binary handlers should use their documented non-JSON response path and may narrowly suppress only this mount when needed.",
                );
            }
        }
    }
}

impl RequestContract {
    fn allows(&self, kind: &str) -> Option<bool> {
        let kinds = self.kinds.as_ref()?;
        if kinds.contains(kind) {
            Some(true)
        } else if self.unresolved_body && matches!(kind, "json" | "form") {
            None
        } else {
            Some(false)
        }
    }
}

fn calls_within_handler<'a>(
    analysis: &'a TsAnalysis,
    mount: &RouteMount,
) -> impl Iterator<Item = &'a CallFact> {
    let end_line = mount.line + mount.handler.lines().count().saturating_sub(1);
    analysis.calls.iter().filter(move |call| {
        call.line >= mount.line
            && call.line <= end_line
            && call.text != mount.handler
            && mount.handler.contains(&call.text)
    })
}

fn check_api_files(project: &Project, app_root: &str, report: &mut Report, rule_id: &'static str) {
    for file in &project.files {
        if !is_ts_source_file(&file.rel_path)
            || is_test_file(&file.rel_path)
            || !has_prefix(&file.rel_path, app_root, "src/server/api/")
        {
            continue;
        }
        let route_file = has_prefix(&file.rel_path, app_root, "src/server/api/routes/")
            && file.rel_path.ends_with(".ts");
        let analysis = file.ts.as_ref();
        let create_routes = analysis
            .map(|analysis| {
                analysis
                    .calls
                    .iter()
                    .filter(|call| is_create_route_call(call))
                    .count()
            })
            .unwrap_or_default();

        if route_file
            && (create_routes == 0
                || !analysis.is_some_and(|analysis| analysis.imports_from("@hono/zod-openapi")))
        {
            report.error(
                rule_id,
                &file.rel_path,
                None,
                "API route module is not a Hono OpenAPI route",
                "Install with `pnpm add hono @hono/zod-openapi zod`. Export createRoute(...) definitions with request/response schemas and mount each symbol once with `app.openapi(route, handler)`.",
            );
        }

        if let Some(analysis) = analysis {
            for call in &analysis.calls {
                if route_file && matches!(call.callee.as_str(), "request.json" | "c.req.json") {
                    report.error(
                        rule_id,
                        &file.rel_path,
                        Some(call.line),
                        "route handler reads JSON without OpenAPI validation",
                        "Declare request.body in createRoute(...) and read it with `c.req.valid('json')`; verify the contract with `pnpm gen:api`.",
                    );
                }
                if route_file && call.callee == "Response.json" {
                    report.error(
                        rule_id,
                        &file.rel_path,
                        Some(call.line),
                        "route handler returns JSON outside the OpenAPI response contract",
                        "Return `c.json(...)` from the handler mounted by app.openapi so literal statuses can be checked against this route's responses. Run `pnpm gen:api` after updating the contract.",
                    );
                }
            }
        }

        for (index, line) in code_only(&file.text).lines().enumerate() {
            if contains_manual_path_router(line) {
                report.error(
                    rule_id,
                    &file.rel_path,
                    Some(index + 1),
                    "manual pathname routing is not allowed in the API layer",
                    "Use an OpenAPIHono app and mount route symbols with app.openapi(...). Keep only the top-level /api bridge in src/server.ts and verify with `pnpm test`.",
                );
            }
        }
    }
}

fn route_is_test_gated(project: &Project, route: &RouteContract) -> bool {
    project.file(&route.file).is_some_and(|file| {
        let code = code_only(&file.text);
        code.contains("APP_ENV") || code.contains("isAuthTestEnvironment")
    })
}

fn path_placeholders(path: &str) -> BTreeSet<String> {
    let mut placeholders = BTreeSet::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find('}') else {
            break;
        };
        let name = &after_open[..close];
        if !name.is_empty()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        {
            placeholders.insert(name.to_string());
        }
        rest = &after_open[close + 1..];
    }
    placeholders
}

fn display_names(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

fn route_label(route: &RouteContract, message: &str) -> String {
    match &route.symbol {
        Some(symbol) => format!("route `{symbol}` {message}"),
        None => message.to_string(),
    }
}

fn route_name(route: &RouteContract) -> String {
    route
        .symbol
        .as_ref()
        .map(|symbol| format!("route `{symbol}`"))
        .unwrap_or_else(|| "createRoute object".to_string())
}

fn mount_in_app(mount: &RouteMount, app_root: &str) -> bool {
    has_prefix(&mount.file, app_root, "src/server/api/")
}

fn referenced_symbol(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '.'))
    {
        return None;
    }
    value.rsplit('.').next().map(str::to_string)
}

fn is_create_route_call(call: &CallFact) -> bool {
    call.callee == "createRoute" || call.callee.ends_with(".createRoute")
}

fn is_openapi_mount(call: &CallFact) -> bool {
    call.callee == "openapi" || call.callee.ends_with(".openapi")
}

fn contains_manual_path_router(line: &str) -> bool {
    line.contains("url.pathname ===")
        || line.contains("url.pathname.startsWith")
        || line.contains("new URL(request.url)")
}

fn is_allowed_api_path(path: &str) -> bool {
    path.starts_with("/api/v1/")
        || path.starts_with("/api/auth/")
        || path == "/api/v1/openapi.json"
        || path == "/api/health"
}

#[cfg(test)]
mod tests {
    use crate::analysis::analyze_typescript;
    use crate::fs::ProjectFile;

    use super::{ParamsContract, parse_route_contract};

    fn route(source: &str) -> super::RouteContract {
        let analysis = analyze_typescript("src/server/api/routes/users.route.ts", source);
        let file = ProjectFile {
            rel_path: "src/server/api/routes/users.route.ts".to_string(),
            text: source.to_string(),
            generated: false,
            ts: Some(analysis.clone()),
        };
        let call = analysis
            .calls
            .iter()
            .find(|call| call.callee == "createRoute")
            .expect("createRoute call");
        parse_route_contract(&file, ".", call).expect("static route")
    }

    #[test]
    fn parses_each_route_object_and_inline_params_independently() {
        let route = route(
            r#"
export const getUser = createRoute({
  method: 'get',
  path: '/api/v1/users/{userId}',
  operationId: 'getUser',
  request: { params: z.object({ userId: z.string() }) },
  responses: { 200: { description: 'ok' }, 404: { description: 'missing' } },
});
"#,
        );

        assert_eq!(route.symbol.as_deref(), Some("getUser"));
        assert_eq!(route.method.as_deref(), Some("get"));
        assert_eq!(route.path.as_deref(), Some("/api/v1/users/{userId}"));
        assert_eq!(route.operation_id.as_deref(), Some("getUser"));
        assert_eq!(route.response_statuses.unwrap(), [200, 404].into());
        assert_eq!(
            route.request.params,
            ParamsContract::Resolved(["userId".to_string()].into())
        );
    }

    #[test]
    fn reports_dynamic_route_fields_as_unresolved_without_borrowing_other_routes() {
        let route = route(
            "export const route = createRoute({ method, path, responses: sharedResponses });",
        );

        assert!(route.method.is_none());
        assert!(route.path.is_none());
        assert!(route.response_statuses.is_none());
    }
}
