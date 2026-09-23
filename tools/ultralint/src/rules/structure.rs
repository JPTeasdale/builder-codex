use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};

pub struct ProjectStructureRule;

impl Rule for ProjectStructureRule {
    fn id(&self) -> &'static str {
        "project-structure"
    }

    fn category(&self) -> &'static str {
        "structure"
    }

    fn description(&self) -> &'static str {
        "enforces the Edge Service Stack file layout, package scripts, and source directory schema"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        check_root(project, config, report, self.id());

        for package_dir in &config.shared_packages {
            check_dir(project, package_dir, report, self.id());
            check_file(
                project,
                &join(package_dir, "package.json"),
                report,
                self.id(),
            );
        }

        for app_root in &config.web_apps {
            check_web_app(project, app_root, report, self.id());
        }
        for app_root in &config.worker_apps {
            check_worker_app(project, app_root, report, self.id());
        }
        for app_root in &config.mobile_apps {
            check_mobile_app(project, app_root, report, self.id());
        }

        for app_root in &config.web_apps {
            check_web_source_schema(project, app_root, report, self.id());
        }
    }
}

fn check_root(project: &Project, config: &Config, report: &mut Report, rule_id: &'static str) {
    check_file(project, "package.json", report, rule_id);
    check_file(project, "tsconfig.json", report, rule_id);

    if project.exists(".ultralint.toml") {
        report.error(
			rule_id,
			".ultralint.toml",
			None,
			"project-local ultralint configuration is not allowed",
			"ultralint is intentionally opinionated; change policy in the plugin/tool, not per project",
		);
    }

    for root_dir in &config.required_root_dirs {
        check_dir(project, root_dir, report, rule_id);
    }

    if uses_workspace_slots(config) {
        let Some(package_json) = project.read("package.json") else {
            return;
        };
        if !package_json.contains("\"workspaces\"") {
            report.error(
                rule_id,
                "package.json",
                None,
                "multi-slot app contract requires package.json workspaces",
                "declare apps/* and packages/* workspaces at the repo root",
            );
        }
    }
}

fn uses_workspace_slots(config: &Config) -> bool {
    config.web_apps.iter().any(|app| app != ".")
        || config.worker_apps.iter().any(|app| app != ".")
        || !config.mobile_apps.is_empty()
        || !config.shared_packages.is_empty()
}

fn check_web_app(project: &Project, app_root: &str, report: &mut Report, rule_id: &'static str) {
    for rel_path in WEB_REQUIRED_FILES {
        check_file(project, &join(app_root, rel_path), report, rule_id);
    }
    for rel_path in WEB_REQUIRED_DIRS {
        check_dir(project, &join(app_root, rel_path), report, rule_id);
    }
    check_package_scripts(project, app_root, report, rule_id);
}

fn check_worker_app(project: &Project, app_root: &str, report: &mut Report, rule_id: &'static str) {
    for rel_path in WORKER_REQUIRED_FILES {
        check_file(project, &join(app_root, rel_path), report, rule_id);
    }
}

fn check_mobile_app(project: &Project, app_root: &str, report: &mut Report, rule_id: &'static str) {
    for rel_path in MOBILE_REQUIRED_FILES {
        check_file(project, &join(app_root, rel_path), report, rule_id);
    }
    for rel_path in MOBILE_REQUIRED_DIRS {
        check_dir(project, &join(app_root, rel_path), report, rule_id);
    }
}

fn check_web_source_schema(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    check_direct_child_dirs(
        project,
        &join(app_root, "src"),
        SRC_ALLOWED_DIRS,
        report,
        rule_id,
        "src/ is limited to server/ (server-only systems), lib/ (shared modules and client-callable functions), schemas/ (generated shared contracts), components/ (React UI), routes/ (TanStack routes), styles/ (global CSS), and tests/.",
    );
    check_direct_child_dirs(
        project,
        &join(app_root, "src/server"),
        SERVER_ALLOWED_DIRS,
        report,
        rule_id,
        "server/ is server-only: api/ mounts /api handlers, auth/ owns Better Auth runtime setup, config/ owns env/runtime config, cloudflare/ owns platform bindings, clients/ wraps external SDKs, domain/ owns business workflows, and db/ owns database access.",
    );
    check_direct_child_dirs(
        project,
        &join(app_root, "src/server/api"),
        API_ALLOWED_DIRS,
        report,
        rule_id,
        "server/api/ contains mounted /api handlers. Use routes/ for endpoint modules and middleware/ for API middleware.",
    );
    check_direct_child_dirs(
        project,
        &join(app_root, "src/server/db"),
        DB_ALLOWED_DIRS,
        report,
        rule_id,
        "server/db/ contains index.ts createDb exports plus schema/ and repositories/. Put business workflows in src/server/domain, not the database layer.",
    );
    check_direct_child_dirs(
        project,
        &join(app_root, "src/components"),
        COMPONENTS_ALLOWED_DIRS,
        report,
        rule_id,
        "components/ contains React UI only: ui/ for shadcn primitives, blocks/ for stateless reusable composed pieces, layout/ for shells/navigation, and pages/ for top-level page workflows including page-specific forms.",
    );
    check_direct_child_dirs(
        project,
        &join(app_root, "src/schemas"),
        SCHEMAS_ALLOWED_DIRS,
        report,
        rule_id,
        "schemas/ is for non-API generated shared contracts. OpenAPI output belongs in src/lib/api/v1.d.ts and is generated with `pnpm gen:api`.",
    );
    check_lib_source_schema(project, app_root, report, rule_id);
    check_tsx_locations(project, app_root, report, rule_id);
    check_forbidden_route_dirs(project, app_root, report, rule_id);
}

fn check_tsx_locations(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    for file in &project.files {
        if !file.rel_path.ends_with(".tsx") {
            continue;
        }
        if !has_prefix(&file.rel_path, app_root, "src/") {
            continue;
        }
        if has_prefix(&file.rel_path, app_root, "src/components/") {
            continue;
        }

        report.error(
            rule_id,
            &file.rel_path,
            None,
            ".tsx files are only allowed under src/components/",
            "keep routes and shared modules as .ts files; put React component implementations under src/components/*",
        );
    }
}

fn check_lib_source_schema(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let lib_dir = project.root.join(join(app_root, "src/lib"));
    let Ok(entries) = fs::read_dir(&lib_dir) else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };

        if file_type.is_dir() {
            if name == "api" {
                continue;
            }
            if name.starts_with('-') && !LIB_GENERIC_DIRS.contains(&name.as_str()) {
                let rel_path = join(app_root, &format!("src/lib/{name}"));
                report.error(
                    rule_id,
                    rel_path,
                    None,
                    format!("generic lib directory `{name}` is not allowed"),
                    "use src/lib/-hooks or src/lib/-schema for generic code; otherwise create src/lib/<module>/<module>.*.ts files. Server workflows belong in src/server/domain and are exposed through Hono OpenAPI routes, not src/lib/-functions.",
                );
                continue;
            }
            if !name.starts_with('-') {
                check_lib_module_files(project, app_root, &name, report, rule_id);
            }
        } else if file_type.is_file() && is_ts_or_tsx(&name) {
            let rel_path = join(app_root, &format!("src/lib/{name}"));
            report.error(
                rule_id,
                rel_path,
                None,
                "direct TypeScript files under src/lib are not allowed",
                "move generic code to src/lib/-hooks or src/lib/-schema; move module code to src/lib/<module>/<module>.*.ts. Server workflows belong in src/server/domain and are exposed through Hono OpenAPI routes.",
            );
        }
    }
}

fn check_lib_module_files(
    project: &Project,
    app_root: &str,
    module: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let module_dir = project
        .root
        .join(join(app_root, &format!("src/lib/{module}")));
    let Ok(entries) = fs::read_dir(&module_dir) else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_ts_or_tsx(&name) || is_allowed_lib_module_file(module, &name) {
            continue;
        }

        let rel_path = join(app_root, &format!("src/lib/{module}/{name}"));
        report.error(
            rule_id,
            rel_path,
            None,
            format!("module file `{name}` does not match the src/lib module suffix contract"),
            "module files must be named <module>.client.ts, <module>.hooks.ts, <module>.schema.ts, or <module>.types.ts. Server workflows belong in src/server/domain and should be exposed through src/server/api/routes/*.route.ts.",
        );
    }
}

fn is_allowed_lib_module_file(module: &str, name: &str) -> bool {
    LIB_MODULE_SUFFIXES
        .iter()
        .any(|suffix| name == format!("{module}{suffix}"))
}

fn is_ts_or_tsx(name: &str) -> bool {
    name.ends_with(".ts") || name.ends_with(".tsx")
}

fn has_prefix(rel_path: &str, root: &str, suffix: &str) -> bool {
    if root == "." || root.is_empty() {
        rel_path.starts_with(suffix)
    } else {
        rel_path.starts_with(&format!("{}/{}", root.trim_end_matches('/'), suffix))
    }
}

fn check_direct_child_dirs(
    project: &Project,
    rel_dir: &str,
    allowed: &[&str],
    report: &mut Report,
    rule_id: &'static str,
    help: &'static str,
) {
    let abs_dir = project.root.join(rel_dir);
    let Ok(entries) = fs::read_dir(&abs_dir) else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if should_ignore_dir(&name) || allowed.contains(&name.as_str()) {
            continue;
        }

        report.error(
            rule_id,
            format!("{rel_dir}/{name}"),
            None,
            format!("directory `{name}` is not allowed directly under `{rel_dir}`"),
            help,
        );
    }
}

fn check_forbidden_route_dirs(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let routes_dir = project.root.join(join(app_root, "src/routes"));
    if !routes_dir.exists() {
        return;
    }
    walk_route_dirs(project, &routes_dir, report, rule_id);
}

fn walk_route_dirs(project: &Project, dir: &Path, report: &mut Report, rule_id: &'static str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if should_ignore_dir(name) {
            continue;
        }

        if FORBIDDEN_ROUTE_DIRS.contains(&name) {
            let rel_path = path
                .strip_prefix(&project.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            report.error(
                rule_id,
                rel_path,
                None,
                format!("route-local `{name}` directory is not allowed"),
                "keep route files thin; move UI to src/components, shared module code to src/lib, generated contracts to src/schemas, and server-only work to src/server.",
            );
            continue;
        }

        walk_route_dirs(project, &path, report, rule_id);
    }
}

fn should_ignore_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules" | "dist" | "build" | "coverage" | "test-results" | "playwright-report"
        )
}

fn check_package_scripts(
    project: &Project,
    app_root: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let package_path = join(app_root, "package.json");
    let Some(package_json) = project.read(&package_path) else {
        return;
    };

    for script in REQUIRED_SCRIPTS {
        let needle = format!("\"{script}\"");
        if !package_json.contains(&needle) {
            report.error(
                rule_id,
                &package_path,
                None,
                format!("required package script `{script}` is missing"),
                format!("add it with `pnpm pkg set scripts.{script}=...`"),
            );
        }
    }

    for script in RECOMMENDED_SCRIPTS {
        let needle = format!("\"{script}\"");
        if !package_json.contains(&needle) {
            report.warning(
                rule_id,
                &package_path,
                None,
                format!("recommended package script `{script}` is missing"),
                "generated contracts are easier to keep current when scripts are standardized",
            );
        }
    }
}

fn check_file(project: &Project, rel_path: &str, report: &mut Report, rule_id: &'static str) {
    if !project.is_file(rel_path) {
        report.error(
            rule_id,
            rel_path,
            None,
            format!("required file {rel_path} is missing"),
            "create the file through the project scaffold starter or the relevant CLI",
        );
    }
}

fn check_dir(project: &Project, rel_path: &str, report: &mut Report, rule_id: &'static str) {
    if !project.is_dir(rel_path) || !project.any_file_under(rel_path) {
        report.error(
            rule_id,
            rel_path,
            None,
            format!("required directory {rel_path} is missing or empty"),
            "keep stack subsystems in their standard shared locations",
        );
    }
}

fn join(root: &str, rel_path: &str) -> String {
    if root == "." || root.is_empty() {
        rel_path.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel_path)
    }
}

const WEB_REQUIRED_FILES: &[&str] = &[
    "package.json",
    "vite.config.ts",
    "drizzle.config.ts",
    "better-auth.config.ts",
    "src/server.ts",
    "src/router.ts",
    "src/routes/__root.ts",
    "src/server/api/index.ts",
    "src/server/auth/better-auth.ts",
    "src/server/config/env.ts",
    "src/server/config/urls.ts",
    "src/server/config/features.ts",
    "src/server/cloudflare/bindings.ts",
    "src/server/db/index.ts",
    "src/lib/api/client.ts",
    "src/styles/0-theme.css",
    "src/styles/base.css",
    "src/styles/shell.css",
];

const WEB_REQUIRED_DIRS: &[&str] = &[
    "src/server/api",
    "src/server/api/routes",
    "src/server/api/middleware",
    "src/server/auth",
    "src/server/config",
    "src/server/cloudflare",
    "src/server/db",
    "src/server/db/schema",
    "src/server/db/repositories",
    "src/lib",
    "src/lib/-hooks",
    "src/lib/-schema",
    "src/routes",
    "src/components",
    "src/components/ui",
    "src/components/blocks",
    "src/components/layout",
    "src/components/pages",
    "src/styles",
    "scripts",
];

const WORKER_REQUIRED_FILES: &[&str] = &["wrangler.jsonc"];

const MOBILE_REQUIRED_FILES: &[&str] = &["package.json", "capacitor.config.ts"];

const MOBILE_REQUIRED_DIRS: &[&str] = &["src"];

const SRC_ALLOWED_DIRS: &[&str] = &[
    "server",
    "lib",
    "schemas",
    "components",
    "routes",
    "styles",
    "tests",
];

const SERVER_ALLOWED_DIRS: &[&str] = &[
    "api",
    "auth",
    "config",
    "cloudflare",
    "clients",
    "domain",
    "db",
];

const COMPONENTS_ALLOWED_DIRS: &[&str] = &["ui", "blocks", "layout", "pages"];

const SCHEMAS_ALLOWED_DIRS: &[&str] = &["api"];

const API_ALLOWED_DIRS: &[&str] = &["routes", "middleware"];

const DB_ALLOWED_DIRS: &[&str] = &["schema", "repositories"];

const LIB_GENERIC_DIRS: &[&str] = &["-hooks", "-schema"];

const LIB_MODULE_SUFFIXES: &[&str] = &[
    ".client.ts",
    ".client.tsx",
    ".hooks.ts",
    ".hooks.tsx",
    ".schema.ts",
    ".types.ts",
];

const FORBIDDEN_ROUTE_DIRS: &[&str] = &["components", "hooks", "schemas", "utils", "lib"];

const REQUIRED_SCRIPTS: &[&str] = &[
    "dev",
    "build",
    "deploy",
    "check",
    "lint",
    "test",
    "db:generate",
    "db:migrate",
    "db:check",
    "db:init",
    "gen:api",
    "gen:auth",
    "gen:cf",
];

const RECOMMENDED_SCRIPTS: &[&str] = &["test:e2e", "lint:fix", "gen:routes"];
