use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_test_file, is_ts_source_file, join};
use crate::rules::{Report, Rule};

pub struct DbAccessLayerRule;

impl Rule for DbAccessLayerRule {
    fn id(&self) -> &'static str {
        "db-access-layer"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "keeps raw database imports and queries inside the database access layer"
    }
    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) || is_test_file(&file.rel_path) {
                continue;
            }

            let db_module_path = is_allowed_db_module_path(&file.rel_path, config);
            let db_query_path = is_allowed_db_query_path(&file.rel_path, config);

            if let Some(analysis) = &file.ts {
                if !db_module_path {
                    for import in &analysis.imports {
                        if imports_database_package(&import.source) {
                            report.error(
                                self.id(),
                                &file.rel_path,
                                Some(import.line),
                                "database package is imported outside the database layer",
                                "move database-specific imports to src/server/db/repositories or src/server/db/schema. Repositories own Drizzle and persistence mechanics; domain services call repositories.",
                            );
                        }
                    }
                }

                if !db_query_path {
                    for call in &analysis.calls {
                        if calls_database_directly(&call.callee) {
                            report.error(
                                self.id(),
                                &file.rel_path,
                                Some(call.line),
                                "raw database query is performed outside the database access layer",
                                "move the query to src/server/db/repositories/*.repository.ts and call that helper from domain or API code",
                            );
                        }
                    }
                }
            }
        }
    }
}

fn imports_database_package(source: &str) -> bool {
    source == "drizzle-orm"
        || source.starts_with("drizzle-orm/")
        || source == "postgres"
        || source.starts_with("postgres/")
        || source.starts_with("@neondatabase/")
}

fn calls_database_directly(callee: &str) -> bool {
    ["db", "context.db", "tx"].iter().any(|receiver| {
        ["select", "insert", "update", "delete", "execute"]
            .iter()
            .any(|method| callee == format!("{receiver}.{method}"))
            || callee.starts_with(&format!("{receiver}.query."))
    })
}

fn is_allowed_db_module_path(rel_path: &str, config: &Config) -> bool {
    is_tooling_or_script_path(rel_path)
        || config
            .web_apps
            .iter()
            .chain(config.worker_apps.iter())
            .any(|root| {
                has_prefix(rel_path, root, "src/server/db/")
                    || has_prefix(rel_path, root, "src/server/auth/")
            })
        || config.shared_packages.iter().any(|root| {
            has_prefix(rel_path, root, "src/server/db/") || has_prefix(rel_path, root, "src/db/")
        })
}

fn is_allowed_db_query_path(rel_path: &str, config: &Config) -> bool {
    is_tooling_or_script_path(rel_path)
        || config
            .web_apps
            .iter()
            .chain(config.worker_apps.iter())
            .any(|root| is_query_layer_path(rel_path, &join(root, "src/server/db/repositories/")))
        || config.shared_packages.iter().any(|root| {
            is_query_layer_path(rel_path, &join(root, "src/db/repositories/"))
                || is_query_layer_path(rel_path, &join(root, "src/server/db/repositories/"))
        })
}

fn is_query_layer_path(rel_path: &str, allowed_prefix: &str) -> bool {
    rel_path.starts_with(allowed_prefix)
}

fn is_tooling_or_script_path(rel_path: &str) -> bool {
    rel_path.ends_with("drizzle.config.ts")
        || rel_path.starts_with("scripts/")
        || rel_path.contains("/scripts/")
}
