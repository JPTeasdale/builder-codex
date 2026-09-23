use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_test_file, is_ts_source_file, join};
use crate::rules::{Report, Rule};

pub struct RuntimeSafetyRule;

impl Rule for RuntimeSafetyRule {
    fn id(&self) -> &'static str {
        "runtime-safety"
    }

    fn category(&self) -> &'static str {
        "runtime"
    }

    fn description(&self) -> &'static str {
        "prevents request-time migrations and fake Env casts"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) {
                continue;
            }

            for (index, line) in file.text.lines().enumerate() {
                let line_number = index + 1;

                if contains_fake_env_cast(line)
                    && !is_test_file(&file.rel_path)
                    && !is_tooling_path(&file.rel_path)
                {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "fake Cloudflare Env cast is not allowed",
                        "Do not use `{} as Env` or `{} as unknown as Env` in runtime code; it hides missing bindings. In tests, build a real helper such as `testEnv({ DATABASE_URL: '...' }) satisfies Env`. The Better Auth CLI config is the exception: better-auth.config.ts may use casts because it runs as generator tooling, not as a Worker request path.",
                    );
                }

                if is_server_source(&file.rel_path, config)
                    && !is_tooling_path(&file.rel_path)
                    && contains_request_time_migration(line)
                {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "request-time migration or seed setup is not allowed",
                        "Do not mutate schema from Worker request paths. Move migrations and schema repair functions to scripts/db-init.ts or generated Drizzle migrations, then run them with `pnpm db:init` or `pnpm db:migrate`.",
                    );
                }
            }
        }
    }
}

fn contains_fake_env_cast(line: &str) -> bool {
    line.contains("{} as Env") || line.contains("{} as unknown as Env")
}

fn is_server_source(rel_path: &str, config: &Config) -> bool {
    config.web_apps.iter().any(|root| {
        has_prefix(rel_path, root, "src/server/") || rel_path == join(root, "src/server.ts")
    })
}

fn contains_request_time_migration(line: &str) -> bool {
    (line.contains("drizzle/") && line.contains("?raw"))
        || line.contains("migrate(")
        || (line.contains("ensure") && (line.contains("Schema") || line.contains("Migration")))
}

fn is_tooling_path(rel_path: &str) -> bool {
    rel_path.starts_with("scripts/")
        || rel_path.contains("/scripts/")
        || rel_path.ends_with("drizzle.config.ts")
        || rel_path.ends_with("better-auth.config.ts")
}
