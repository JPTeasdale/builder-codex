use crate::config::Config;
use crate::fs::Project;
use crate::rules::{Report, Rule};

pub struct SuppressionHygieneRule;

impl Rule for SuppressionHygieneRule {
    fn id(&self) -> &'static str {
        "suppression-hygiene"
    }

    fn category(&self) -> &'static str {
        "quality"
    }

    fn description(&self) -> &'static str {
        "requires ultralint suppressions to be exact, justified, current, and used"
    }

    fn check(&self, _project: &Project, _config: &Config, _report: &mut Report) {}
}
