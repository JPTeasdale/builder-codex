use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};

pub struct ShadcnUiPrimitivesRule;

impl Rule for ShadcnUiPrimitivesRule {
    fn id(&self) -> &'static str {
        "shadcn-ui-primitives"
    }

    fn category(&self) -> &'static str {
        "react"
    }

    fn description(&self) -> &'static str {
        "requires native button, select, and form elements to be encapsulated by shadcn UI components"
    }

    fn check(&self, project: &Project, _config: &Config, report: &mut Report) {
        for file in &project.files {
            if is_ui_component(&file.rel_path) {
                continue;
            }
            let Some(ts) = &file.ts else {
                continue;
            };

            for element in &ts.jsx_elements {
                let Some(component) = shadcn_component(&element.name) else {
                    continue;
                };
                report.error(
                    self.id(),
                    &file.rel_path,
                    Some(element.line),
                    format!(
                        "native `<{}>` elements are only allowed in src/components/ui",
                        element.name
                    ),
                    shadcn_help(&element.name, component),
                );
            }
        }
    }
}

fn is_ui_component(rel_path: &str) -> bool {
    rel_path
        .split('/')
        .collect::<Vec<_>>()
        .windows(2)
        .any(|parts| parts == ["components", "ui"])
}

fn shadcn_component(element: &str) -> Option<&'static str> {
    match element {
        "button" => Some("Button"),
        "select" => Some("Select"),
        "form" => Some("Form"),
        _ => None,
    }
}

fn shadcn_help(element: &str, component: &str) -> String {
    if element == "form" {
        return "use the shadcn form components and expose a semantic wrapper such as `FormRoot` from `src/components/ui/form.tsx`; keep the native `<form>` implementation inside that UI primitive so pages compose the shadcn form API without rendering raw form markup".to_string();
    }

    format!(
        "use the shadcn `{component}` component from `src/components/ui/{element}.tsx`; add that shadcn component to `src/components/ui` first if it is missing"
    )
}

#[cfg(test)]
mod tests {
    use super::{is_ui_component, shadcn_component, shadcn_help};

    #[test]
    fn maps_only_restricted_native_elements() {
        assert_eq!(shadcn_component("button"), Some("Button"));
        assert_eq!(shadcn_component("select"), Some("Select"));
        assert_eq!(shadcn_component("form"), Some("Form"));
        assert_eq!(shadcn_component("Button"), None);
        assert_eq!(shadcn_component("x-button"), None);
    }

    #[test]
    fn allows_ui_components_in_root_and_nested_packages() {
        assert!(is_ui_component("src/components/ui/button.tsx"));
        assert!(is_ui_component("apps/mobile/src/components/ui/button.tsx"));
        assert!(is_ui_component(
            "packages/design-system/components/ui/button.tsx"
        ));
        assert!(!is_ui_component("src/components/pages/button.tsx"));
    }

    #[test]
    fn form_help_keeps_native_markup_behind_a_ui_wrapper() {
        let help = shadcn_help("form", "Form");
        assert!(help.contains("shadcn form components"));
        assert!(help.contains("FormRoot"));
        assert!(help.contains("native `<form>` implementation"));
    }
}
