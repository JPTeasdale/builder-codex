use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_ts_source_file};
use crate::rules::{Report, Rule};

pub struct NamingComplexityRule;

impl Rule for NamingComplexityRule {
    fn id(&self) -> &'static str {
        "naming-complexity"
    }

    fn category(&self) -> &'static str {
        "architecture"
    }

    fn description(&self) -> &'static str {
        "enforces canonical server filenames and flags oversized backend modules"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) {
                continue;
            }

            for root in &config.web_apps {
                check_naming(&file.rel_path, root, report, self.id());
                check_size(file, root, report, self.id());
            }
        }
    }
}

fn check_naming(rel_path: &str, root: &str, report: &mut Report, rule_id: &'static str) {
    if has_prefix(rel_path, root, "src/server/db/repositories/")
        && !rel_path.ends_with(".repository.ts")
    {
        report.error(
            rule_id,
            rel_path,
            None,
            "repository file must use the .repository.ts suffix",
            "Rename repository files to src/server/db/repositories/<resource>.repository.ts so persistence adapters are obvious at import sites.",
        );
    }

    if has_prefix(rel_path, root, "src/server/api/routes/") && !rel_path.ends_with(".route.ts") {
        report.error(
            rule_id,
            rel_path,
            None,
            "API route file must use the .route.ts suffix",
            "Rename Hono OpenAPI route modules to src/server/api/routes/<resource>.route.ts. Keep routing definitions in route files and mount them from src/server/api/index.ts.",
        );
    }

    if has_prefix(rel_path, root, "src/server/domain/")
        && rel_path.contains("service")
        && !rel_path.ends_with(".service.ts")
    {
        report.error(
            rule_id,
            rel_path,
            None,
            "domain workflow file must use the .service.ts suffix",
            "Workflow/orchestration modules in src/server/domain should be named <name>.service.ts. Pure helpers may use descriptive names such as application-normalization.ts.",
        );
    }
}

fn check_size(
    file: &crate::fs::ProjectFile,
    root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let lines = file.text.lines().count();
    if has_prefix(&file.rel_path, root, "src/server/db/schema/") {
        if !file.rel_path.contains(".gen.") && lines > 600 {
            report.warning(
                rule_id,
                &file.rel_path,
                None,
                format!("Drizzle schema file has {lines} lines; consider splitting before it becomes a mega-schema"),
                "Move unrelated tables into separate schema modules and keep defaults, labels, resume parsing, and validation outside Drizzle schema files.",
            );
        }
        return;
    }
    if has_prefix(&file.rel_path, root, "src/server/api/routes/") && lines > 250 {
        report.warning(
            rule_id,
            &file.rel_path,
            None,
            format!("API route module has {lines} lines; expected 250 or fewer"),
            "Split large Hono route modules by resource/action and keep validation schemas close to each route.",
        );
    } else if has_prefix(&file.rel_path, root, "src/server/") && lines > 400 {
        report.warning(
            rule_id,
            &file.rel_path,
            None,
            format!("backend module has {lines} lines; expected 400 or fewer"),
            "Split large backend files into API routes, domain services, repositories, clients, and shared contracts. Schema files may be larger, but consider splitting them around 600 lines.",
        );
    }
}
