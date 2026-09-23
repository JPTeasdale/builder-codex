use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};

pub struct SecretBoundariesRule;

impl Rule for SecretBoundariesRule {
    fn id(&self) -> &'static str {
        "secret-boundaries"
    }

    fn category(&self) -> &'static str {
        "secrets"
    }

    fn description(&self) -> &'static str {
        "enforces runtime/build/public secret boundaries across web, worker, and mobile slots"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            let frontend = is_frontend_file(&file.rel_path, config);
            let source = is_source_file(&file.rel_path, config);
            let wrangler = file.rel_path.ends_with("wrangler.jsonc")
                || file.rel_path.ends_with("wrangler.toml");

            for (index, line) in file.text.lines().enumerate() {
                let line_number = index + 1;

                for secret in &config.runtime_secrets {
                    if !line.contains(secret) {
                        continue;
                    }
                    if frontend || uses_public_env(line, secret, config) {
                        report.error(
							self.id(),
							&file.rel_path,
							Some(line_number),
							format!("runtime secret `{secret}` is referenced in client/mobile code"),
							"runtime secrets must stay behind Worker Env bindings and server-only code",
						);
                    }
                    if wrangler && looks_like_var_assignment(line, secret) {
                        report.error(
							self.id(),
							&file.rel_path,
							Some(line_number),
							format!("runtime secret `{secret}` appears to be configured as a public Worker var"),
							"put runtime secrets in Wrangler secrets or `secrets.required`, not `vars`",
						);
                    }
                }

                for secret in &config.build_secrets {
                    if !line.contains(secret) {
                        continue;
                    }
                    if source || frontend || uses_public_env(line, secret, config) {
                        report.error(
							self.id(),
							&file.rel_path,
							Some(line_number),
							format!("build secret `{secret}` is referenced in runtime/client source"),
							"build secrets belong in CI/build tooling, not Worker runtime, browser, or mobile bundles",
						);
                    }
                    if wrangler && looks_like_var_assignment(line, secret) {
                        report.error(
                            self.id(),
                            &file.rel_path,
                            Some(line_number),
                            format!(
                                "build secret `{secret}` appears to be configured as a Worker var"
                            ),
                            "build secrets should live in CI/build secret storage only",
                        );
                    }
                }

                for prefix in &config.public_env_prefixes {
                    if line.contains(prefix) && looks_sensitive(line) {
                        report.warning(
							self.id(),
							&file.rel_path,
							Some(line_number),
							format!("public env prefix `{prefix}` is used with a sensitive-looking name"),
							"public env names containing SECRET, TOKEN, PRIVATE, PASSWORD, or KEY should be reviewed",
						);
                    }
                }
            }
        }
    }
}

fn is_frontend_file(rel_path: &str, config: &Config) -> bool {
    config.web_apps.iter().any(|root| {
        has_prefix(rel_path, root, "src/routes/") || has_prefix(rel_path, root, "src/components/")
    }) || config
        .mobile_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/"))
}

fn is_source_file(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/"))
        || config
            .worker_apps
            .iter()
            .any(|root| has_prefix(rel_path, root, "src/"))
        || config
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

fn uses_public_env(line: &str, secret: &str, config: &Config) -> bool {
    config.public_env_prefixes.iter().any(|prefix| {
        line.contains(&format!("{prefix}{secret}"))
            || line.contains(&format!("import.meta.env.{secret}"))
    })
}

fn looks_like_var_assignment(line: &str, secret: &str) -> bool {
    if line.contains("\"required\"") {
        return false;
    }
    line.contains(&format!("\"{secret}\"")) && line.contains(':')
}

fn looks_sensitive(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    ["SECRET", "TOKEN", "PRIVATE", "PASSWORD", "KEY"]
        .iter()
        .any(|word| upper.contains(word))
}
