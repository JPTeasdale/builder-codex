use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{
    DeclarationKind, has_path_segment, is_ts_source_file, parse_top_level_declaration,
};
use crate::rules::{Report, Rule};

pub struct HookLocationRule;

impl Rule for HookLocationRule {
    fn id(&self) -> &'static str {
        "hook-location"
    }

    fn category(&self) -> &'static str {
        "react"
    }

    fn description(&self) -> &'static str {
        "requires React hooks to live in lib/-hooks or module .hooks files"
    }
    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) || is_allowed_hook_path(&file.rel_path) {
                continue;
            }

            for (index, line) in file.text.lines().enumerate() {
                let Some(decl) = parse_top_level_declaration(line, &file.rel_path, index + 1)
                else {
                    continue;
                };

                if is_hook_declaration(&decl.kind, &decl.name) {
                    report.error(
                        self.id(),
                        &decl.file,
                        Some(decl.line),
                        format!(
                            "React hook `{}` is declared outside a hooks directory",
                            decl.name
                        ),
                        "move this hook to src/lib/-hooks/* for unowned generic hooks or src/lib/<module>/<module>.hooks.ts for module-owned hooks",
                    );
                }
            }
        }
    }
}

fn is_hook_declaration(kind: &DeclarationKind, name: &str) -> bool {
    matches!(kind, DeclarationKind::Function | DeclarationKind::Const)
        && name.starts_with("use")
        && name
            .chars()
            .nth(3)
            .is_some_and(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
}

fn is_allowed_hook_path(rel_path: &str) -> bool {
    has_path_segment(rel_path, "-hooks")
        || has_path_segment(rel_path, "hooks")
        || rel_path.ends_with(".hooks.ts")
        || rel_path.ends_with(".hooks.tsx")
}
