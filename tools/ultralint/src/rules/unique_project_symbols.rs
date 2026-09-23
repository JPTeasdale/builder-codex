use std::collections::BTreeMap;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{
    Declaration, is_test_file, is_ts_source_file, parse_top_level_declaration,
};
use crate::rules::{Report, Rule};

pub struct UniqueProjectSymbolsRule;

impl Rule for UniqueProjectSymbolsRule {
    fn id(&self) -> &'static str {
        "unique-project-symbols"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "prevents duplicate top-level functions, values, classes, and types across the project"
    }
    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        let mut declarations = Vec::new();

        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) || is_test_file(&file.rel_path) {
                continue;
            }

            for (index, line) in file.text.lines().enumerate() {
                let Some(decl) = parse_top_level_declaration(line, &file.rel_path, index + 1)
                else {
                    continue;
                };

                if !decl.exported || is_allowed_duplicate_name(&decl.name) {
                    continue;
                }
                declarations.push(decl);
            }
        }

        let mut by_name: BTreeMap<&str, Vec<&Declaration>> = BTreeMap::new();
        for decl in &declarations {
            by_name.entry(&decl.name).or_default().push(decl);
        }

        for (name, matches) in by_name {
            if matches.len() < 2 {
                continue;
            }

            report.error(
                self.id(),
                &matches[0].file,
                Some(matches[0].line),
                format!("project symbol `{name}` is declared multiple times"),
                format!(
                    "reuse the existing function/type/value or rename this declaration to express a distinct responsibility: {}",
                    join_locations(&matches)
                ),
            );
        }
    }
}

fn join_locations(decls: &[&Declaration]) -> String {
    decls
        .iter()
        .map(|decl| {
            let exported = if decl.exported { "exported " } else { "" };
            format!(
                "{}:{} ({}{})",
                decl.file,
                decl.line,
                exported,
                decl.kind.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_allowed_duplicate_name(name: &str) -> bool {
    matches!(
        name,
        "Route"
            | "loader"
            | "component"
            | "pendingComponent"
            | "errorComponent"
            | "notFoundComponent"
            | "ErrorComponent"
            | "PendingComponent"
            | "NotFoundComponent"
            | "head"
            | "headers"
            | "meta"
            | "links"
    )
}
