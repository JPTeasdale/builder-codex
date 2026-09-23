use std::path::Path;
use std::process::Command;

use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};

pub struct TrackedSecretFilesRule;

impl Rule for TrackedSecretFilesRule {
    fn id(&self) -> &'static str {
        "tracked-secret-files"
    }

    fn category(&self) -> &'static str {
        "secrets"
    }

    fn description(&self) -> &'static str {
        "rejects git-tracked environment and Worker development secret files without reading or exposing their contents"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        let Some(paths) = git_tracked_paths(&project.root) else {
            return;
        };
        for path in paths {
            if !is_forbidden_secret_path(&path) {
                continue;
            }
            report.error(
                self.id(),
                &path,
                None,
                "secret-bearing environment file is tracked by git",
                "Remove the file from the git index without printing it (`git rm --cached -- <path>`), add the matching .env/.dev.vars pattern to .gitignore, rotate every credential that may have been committed, and purge history through the repository's approved incident process when required. Commit only suffix-safe placeholders such as .env.example, .env.template, .dev.vars.example, or .dev.vars.template with inert values.",
            );
        }
    }
}

fn git_tracked_paths(root: &Path) -> Option<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--", "."])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(
        output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| {
                String::from_utf8_lossy(path)
                    .trim_start_matches("./")
                    .to_string()
            })
            .collect(),
    )
}

fn is_forbidden_secret_path(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    let secret_shape =
        file_name == ".env" || file_name.starts_with(".env.") || file_name.starts_with(".dev.vars");
    secret_shape && !is_safe_template(file_name)
}

fn is_safe_template(file_name: &str) -> bool {
    file_name.ends_with(".example") || file_name.ends_with(".template")
}

#[cfg(test)]
mod tests {
    use super::is_forbidden_secret_path;

    #[test]
    fn rejects_secret_files_in_any_tracked_directory() {
        assert!(is_forbidden_secret_path(".env"));
        assert!(is_forbidden_secret_path("apps/web/.env.production"));
        assert!(is_forbidden_secret_path("apps/web/.dev.vars.local"));
    }

    #[test]
    fn permits_only_example_and_template_suffixes() {
        assert!(!is_forbidden_secret_path(".env.example"));
        assert!(!is_forbidden_secret_path("apps/web/.env.local.template"));
        assert!(!is_forbidden_secret_path(".dev.vars.example"));
        assert!(is_forbidden_secret_path(".env.example.local"));
    }
}
