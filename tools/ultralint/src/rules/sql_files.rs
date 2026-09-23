use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::fs::{Project, normalize};
use crate::rules::{Report, Rule};

pub struct SqlFilesRule;

impl Rule for SqlFilesRule {
    fn id(&self) -> &'static str {
        "sql-files"
    }

    fn category(&self) -> &'static str {
        "database"
    }

    fn description(&self) -> &'static str {
        "requires SQL files to be generated Drizzle migrations under drizzle/*.sql"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        let mut sql_files = Vec::new();
        collect_sql_files(&project.root, &project.root, &mut sql_files);

        for rel_path in sql_files {
            if is_direct_drizzle_sql(&rel_path) {
                continue;
            }

            report.error(
                self.id(),
                rel_path,
                None,
                "SQL files are only allowed under drizzle/*.sql",
                "SQL files should only be generated using Drizzle migrations and should not live anywhere else in the codebase",
            );
        }
    }
}

fn collect_sql_files(root: &Path, dir: &Path, sql_files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            collect_sql_files(root, &path, sql_files);
            continue;
        }

        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("sql") {
            continue;
        }

        sql_files.push(normalize(path.strip_prefix(root).unwrap_or(&path)));
    }
}

fn is_direct_drizzle_sql(rel_path: &str) -> bool {
    let path = PathBuf::from(rel_path);
    let Some(parent) = path.parent() else {
        return false;
    };
    parent.file_name().and_then(|name| name.to_str()) == Some("drizzle")
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git"
            | ".tanstack"
            | ".wrangler"
            | "build"
            | "coverage"
            | "dist"
            | "node_modules"
            | "playwright-report"
            | "test-results"
    )
}
