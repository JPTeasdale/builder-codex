use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::analysis::{TsAnalysis, analyze_typescript};

#[derive(Debug, Clone)]
pub struct ProjectFile {
    pub rel_path: String,
    pub text: String,
    pub generated: bool,
    pub ts: Option<TsAnalysis>,
}

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub files: Vec<ProjectFile>,
    aliases: Vec<PathAlias>,
}

#[derive(Debug)]
struct PathAlias {
    app_root: String,
    source_prefix: String,
    source_suffix: String,
    target_prefix: String,
    target_suffix: String,
}

impl Project {
    pub fn load(root: PathBuf) -> io::Result<Self> {
        let mut files = Vec::new();
        walk(&root, &root, &mut files)?;
        let aliases = collect_aliases(&files);
        Ok(Self {
            root,
            files,
            aliases,
        })
    }

    pub fn exists(&self, rel_path: &str) -> bool {
        self.root.join(rel_path).exists()
    }

    pub fn is_file(&self, rel_path: &str) -> bool {
        self.root.join(rel_path).is_file()
    }

    pub fn is_dir(&self, rel_path: &str) -> bool {
        self.root.join(rel_path).is_dir()
    }

    pub fn read(&self, rel_path: &str) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.rel_path == rel_path)
            .map(|file| file.text.as_str())
    }

    pub fn any_file_under(&self, rel_prefix: &str) -> bool {
        let prefix = format!("{}/", rel_prefix.trim_end_matches('/'));
        self.files
            .iter()
            .any(|file| file.rel_path.starts_with(&prefix))
    }

    pub fn file(&self, rel_path: &str) -> Option<&ProjectFile> {
        self.files.iter().find(|file| file.rel_path == rel_path)
    }

    pub fn resolve_import(&self, from: &str, source: &str) -> Option<String> {
        if source.starts_with('.') {
            let parent = Path::new(from).parent().unwrap_or_else(|| Path::new(""));
            return Some(self.resolve_candidate(clean_path(&parent.join(source))));
        }

        let alias = self
            .aliases
            .iter()
            .filter(|alias| is_within_root(from, &alias.app_root))
            .filter_map(|alias| {
                let middle = source
                    .strip_prefix(&alias.source_prefix)?
                    .strip_suffix(&alias.source_suffix)?;
                Some((alias.app_root.len(), alias, middle))
            })
            .max_by_key(|(root_len, _, _)| *root_len);
        if let Some((_, alias, middle)) = alias {
            let separator = if alias.target_prefix.is_empty()
                || alias.target_prefix.ends_with('/')
                || middle.is_empty()
            {
                ""
            } else {
                "/"
            };
            let target = format!(
                "{}{separator}{}{}",
                alias.target_prefix, middle, alias.target_suffix
            );
            return Some(self.resolve_candidate(clean_path(Path::new(&target))));
        }

        let app_root = app_root_for_file(from);
        for prefix in ["~/", "@/"] {
            if let Some(rest) = source.strip_prefix(prefix) {
                let target = join_root(&app_root, &format!("src/{rest}"));
                return Some(self.resolve_candidate(target));
            }
        }
        if source.starts_with("src/") {
            return Some(self.resolve_candidate(join_root(&app_root, source)));
        }
        None
    }

    fn resolve_candidate(&self, candidate: String) -> String {
        for path in [
            candidate.clone(),
            format!("{candidate}.ts"),
            format!("{candidate}.tsx"),
            format!("{candidate}.d.ts"),
            format!("{candidate}/index.ts"),
            format!("{candidate}/index.tsx"),
        ] {
            if self.is_file(&path) {
                return path;
            }
        }
        candidate
    }
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<ProjectFile>) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            walk(root, &path, files)?;
            continue;
        }

        if !metadata.is_file() {
            continue;
        }

        let rel_path = normalize(path.strip_prefix(root).unwrap_or(&path));
        if !should_read_file(&rel_path) {
            continue;
        }

        let text = fs::read_to_string(&path)?;
        let generated = is_generated(&rel_path);
        let ts = is_typescript(&rel_path).then(|| analyze_typescript(&rel_path, &text));
        files.push(ProjectFile {
            rel_path,
            text,
            generated,
            ts,
        });
    }

    Ok(())
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
            | "drizzle"
            | "node_modules"
            | "playwright-report"
            | "test-results"
    )
}

fn should_read_file(rel_path: &str) -> bool {
    is_typescript(rel_path)
        || rel_path.ends_with(".tsx")
        || rel_path.ends_with(".yml")
        || rel_path.ends_with(".yaml")
        || rel_path.ends_with(".json")
        || rel_path.ends_with(".jsonc")
        || rel_path.ends_with(".css")
        || rel_path.ends_with(".md")
}

fn is_typescript(rel_path: &str) -> bool {
    rel_path.ends_with(".ts") || rel_path.ends_with(".tsx")
}

pub fn is_generated(rel_path: &str) -> bool {
    rel_path.ends_with(".d.ts")
        || rel_path.contains(".gen.")
        || rel_path.ends_with("routeTree.gen.ts")
        || rel_path.ends_with("worker-configuration.d.ts")
}

pub fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_aliases(files: &[ProjectFile]) -> Vec<PathAlias> {
    let mut aliases = Vec::new();
    for file in files {
        if !file.rel_path.ends_with("tsconfig.json") {
            continue;
        }
        let Ok(config) = crate::structured::parse_jsonc(&file.text) else {
            continue;
        };
        let Some(compiler) = config.get("compilerOptions") else {
            continue;
        };
        let base_url = compiler
            .get("baseUrl")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(".");
        let Some(paths) = compiler.get("paths").and_then(serde_json::Value::as_object) else {
            continue;
        };
        let app_root = Path::new(&file.rel_path)
            .parent()
            .map(normalize)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| ".".to_string());
        for (source, targets) in paths {
            let Some(target) = targets
                .as_array()
                .and_then(|targets| targets.first())
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let (source_prefix, source_suffix) = split_wildcard(source);
            let (target_prefix, target_suffix) = split_wildcard(target);
            let target_prefix =
                clean_path(&Path::new(&app_root).join(base_url).join(target_prefix));
            aliases.push(PathAlias {
                app_root: app_root.clone(),
                source_prefix: source_prefix.to_string(),
                source_suffix: source_suffix.to_string(),
                target_prefix,
                target_suffix: target_suffix.to_string(),
            });
        }
    }
    aliases.sort_by(|left, right| {
        right
            .source_prefix
            .len()
            .cmp(&left.source_prefix.len())
            .then_with(|| left.app_root.cmp(&right.app_root))
    });
    aliases
}

fn split_wildcard(value: &str) -> (&str, &str) {
    value.split_once('*').unwrap_or((value, ""))
}

fn clean_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

fn app_root_for_file(rel_path: &str) -> String {
    let parts = rel_path.split('/').collect::<Vec<_>>();
    if parts.first() == Some(&"apps") && parts.len() > 2 {
        format!("apps/{}", parts[1])
    } else {
        ".".to_string()
    }
}

fn is_within_root(rel_path: &str, root: &str) -> bool {
    root == "." || rel_path.starts_with(&format!("{root}/"))
}

fn join_root(root: &str, rel_path: &str) -> String {
    if root == "." {
        rel_path.to_string()
    } else {
        format!("{root}/{rel_path}")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::Project;

    #[test]
    fn resolves_aliases_and_relative_imports() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ultralint-resolve-{stamp}"));
        fs::create_dir_all(root.join("src/server/domain")).expect("dirs");
        fs::create_dir_all(root.join("src/components")).expect("dirs");
        fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"~/*":["./src/*"]}}}"#,
        )
        .expect("tsconfig");
        fs::write(root.join("src/server/domain/service.ts"), "export {};").expect("server");
        fs::write(root.join("src/components/a.ts"), "export {};").expect("component");
        let project = Project::load(root.clone()).expect("project");
        assert_eq!(
            project.resolve_import("src/components/a.ts", "~/server/domain/service"),
            Some("src/server/domain/service.ts".to_string())
        );
        assert_eq!(
            project.resolve_import("src/components/a.ts", "../server/domain/service"),
            Some("src/server/domain/service.ts".to_string())
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
