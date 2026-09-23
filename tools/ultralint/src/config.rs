use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub web_apps: Vec<String>,
    pub mobile_apps: Vec<String>,
    pub worker_apps: Vec<String>,
    pub shared_packages: Vec<String>,
    pub required_root_dirs: Vec<String>,
    pub runtime_secrets: BTreeSet<String>,
    pub build_secrets: BTreeSet<String>,
    pub public_env_prefixes: Vec<String>,
    pub allowed_type_names: BTreeSet<String>,
}

impl Config {
    pub fn load(root: &Path) -> Self {
        let mut policy = Self::default();

        let app_dirs = child_dirs(root, "apps");
        if !app_dirs.is_empty() {
            policy.web_apps = detect_react_web_apps(root, &app_dirs);
            policy.worker_apps = app_dirs
                .iter()
                .filter(|app| is_worker_app(root, app))
                .cloned()
                .collect();
            policy.mobile_apps = app_dirs
                .iter()
                .filter(|app| is_mobile_app(root, app))
                .cloned()
                .collect();
            policy.shared_packages = shared_package_dirs(root);
            policy.required_root_dirs = vec!["docs".to_string(), "infra".to_string()];
        }

        policy
    }
}

impl Default for Config {
    fn default() -> Self {
        let allowed_type_names = [
            "Props",
            "State",
            "Options",
            "Params",
            "FormValues",
            "Register",
            "Env",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();

        Self {
            web_apps: vec![".".to_string()],
            mobile_apps: Vec::new(),
            worker_apps: vec![".".to_string()],
            shared_packages: Vec::new(),
            required_root_dirs: vec!["docs".to_string()],
            runtime_secrets: [
                "BETTER_AUTH_SECRET",
                "POSTHOG_API_KEY",
                "OPENAI_API_KEY",
                "STRIPE_SECRET_KEY",
                "STRIPE_WEBHOOK_SECRET",
                "WEBHOOK_SIGNING_SECRET",
                "TELEGRAM_BOT_TOKEN",
                "TELEGRAM_WEBHOOK_SECRET",
                "AWS_SECRET_ACCESS_KEY",
                "RESEND_API_KEY",
                "SENDGRID_API_KEY",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            build_secrets: [
                "CLOUDFLARE_API_TOKEN",
                "NEON_API_KEY",
                "SENTRY_AUTH_TOKEN",
                "TURBO_TOKEN",
                "VERCEL_TOKEN",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            public_env_prefixes: vec!["VITE_".to_string(), "PUBLIC_".to_string()],
            allowed_type_names,
        }
    }
}

fn child_dirs(root: &Path, rel_dir: &str) -> Vec<String> {
    let dir = root.join(rel_dir);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut dirs = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .map(|name| format!("{rel_dir}/{name}"))
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

fn shared_package_dirs(root: &Path) -> Vec<String> {
    child_dirs(root, "packages")
        .into_iter()
        .filter(|package| root.join(package).join("package.json").exists())
        .collect()
}

fn detect_react_web_apps(root: &Path, app_dirs: &[String]) -> Vec<String> {
    app_dirs
        .iter()
        .filter(|app| is_web_app(root, app))
        .cloned()
        .collect()
}

fn is_web_app(root: &Path, app: &str) -> bool {
    let app_root = root.join(app);
    app_root.join("vite.config.ts").exists()
        || app_root.join("src/routes").exists()
        || app_root.join("src/router.ts").exists()
        || app_root.join("src/router.tsx").exists()
}

fn is_mobile_app(root: &Path, app: &str) -> bool {
    root.join(app).join("capacitor.config.ts").exists()
}

fn is_worker_app(root: &Path, app: &str) -> bool {
    root.join(app).join("wrangler.jsonc").is_file()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::Config;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ultralint-config-{name}-{stamp}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn workspace_roles_are_detected_independently() {
        let root = temp_root("roles");
        fs::create_dir_all(root.join("apps/web/src/routes")).expect("web dirs");
        fs::write(root.join("apps/web/vite.config.ts"), "").expect("vite");
        fs::create_dir_all(root.join("apps/worker")).expect("worker dirs");
        fs::write(root.join("apps/worker/wrangler.jsonc"), "{}").expect("wrangler");
        fs::create_dir_all(root.join("apps/mobile")).expect("mobile dirs");
        fs::write(root.join("apps/mobile/capacitor.config.ts"), "").expect("capacitor");

        let config = Config::load(&root);
        assert_eq!(config.web_apps, ["apps/web"]);
        assert_eq!(config.worker_apps, ["apps/worker"]);
        assert_eq!(config.mobile_apps, ["apps/mobile"]);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
