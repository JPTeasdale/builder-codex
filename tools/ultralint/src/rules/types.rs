use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};

pub struct TypeContractsRule;

#[derive(Debug, Clone)]
struct TypeDecl {
    kind: String,
    name: String,
    file: String,
    line: usize,
}

impl Rule for TypeContractsRule {
    fn id(&self) -> &'static str {
        "type-contracts"
    }

    fn category(&self) -> &'static str {
        "types"
    }

    fn description(&self) -> &'static str {
        "prevents frontend-local domain and API contract declarations"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_file(&file.rel_path) {
                continue;
            }
            let frontend = is_frontend_file(&file.rel_path, config);

            for (index, line) in file.text.lines().enumerate() {
                let line_number = index + 1;
                if let Some(decl) = parse_type_declaration(line, &file.rel_path, line_number) {
                    if frontend
                        && !is_presentational_type(&decl.name)
                        && !config.allowed_type_names.contains(&decl.name)
                    {
                        let message =
                            format!("{} `{}` is declared in frontend code", decl.kind, decl.name);
                        let help = "import generated/shared domain types instead of redefining them in routes/components";
                        report.error(self.id(), &decl.file, Some(decl.line), message, help);
                    }

                    if frontend && is_api_contract_name(&decl.name) {
                        report.error(
							self.id(),
							&decl.file,
							Some(decl.line),
							format!("`{}` looks like a frontend-local API contract", decl.name),
							"import the type from generated OpenAPI paths or a shared schema module",
						);
                    }
                }

                if frontend
                    && (line.contains("function ") || line.contains("const "))
                    && line.contains(": {")
                {
                    report.warning(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "component appears to use an inline object type",
                        "use a named Props type or import a shared/generated type",
                    );
                }
            }
        }
    }
}

fn parse_type_declaration(line: &str, file: &str, line_number: usize) -> Option<TypeDecl> {
    let mut trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("export ") {
        trimmed = rest.trim_start();
    }
    if let Some(rest) = trimmed.strip_prefix("declare ") {
        trimmed = rest.trim_start();
    }

    for kind in ["interface", "type", "enum"] {
        let Some(rest) = trimmed.strip_prefix(kind) else {
            continue;
        };
        if !rest.chars().next().is_some_and(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect();
        if !name.is_empty() {
            return Some(TypeDecl {
                kind: kind.to_string(),
                name,
                file: file.to_string(),
                line: line_number,
            });
        }
    }

    None
}

fn is_ts_file(rel_path: &str) -> bool {
    rel_path.ends_with(".ts") || rel_path.ends_with(".tsx")
}

fn is_frontend_file(rel_path: &str, config: &Config) -> bool {
    config.web_apps.iter().any(|root| {
        has_prefix(rel_path, root, "src/routes/") || has_prefix(rel_path, root, "src/components/")
    }) || config
        .mobile_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/"))
}

fn has_prefix(rel_path: &str, root: &str, suffix: &str) -> bool {
    if root == "." || root.is_empty() {
        rel_path.starts_with(suffix)
    } else {
        rel_path.starts_with(&format!("{}/{}", root.trim_end_matches('/'), suffix))
    }
}

fn is_presentational_type(name: &str) -> bool {
    if matches!(name, "RouterContext" | "RouteContext") {
        return true;
    }

    [
        "Props",
        "State",
        "Options",
        "Params",
        "FormValues",
        "Field",
        "Fields",
        "Config",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

fn is_api_contract_name(name: &str) -> bool {
    ["Response", "Request", "Payload", "Dto", "DTO", "Result"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::parse_type_declaration;

    #[test]
    fn typeof_expression_is_not_a_type_declaration() {
        assert!(
            parse_type_declaration("if (typeof value === 'object') {", "route.ts", 1).is_none()
        );
    }
}
