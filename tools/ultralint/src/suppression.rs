use std::collections::BTreeSet;

use chrono::{NaiveDate, Utc};

use crate::fs::Project;

const MARKER: &str = "ultralint: allow ";

#[derive(Debug, Clone)]
pub struct Suppression {
    pub path: String,
    pub line: usize,
    pub rule_id: String,
    pub reason: String,
    pub expires: Option<NaiveDate>,
    pub used: bool,
}

#[derive(Debug, Clone)]
pub struct SuppressionDiagnostic {
    pub path: String,
    pub line: usize,
    pub message: String,
    pub help: String,
}

#[derive(Debug, Clone, Default)]
pub struct SuppressionSet {
    pub entries: Vec<Suppression>,
    pub diagnostics: Vec<SuppressionDiagnostic>,
}

impl SuppressionSet {
    pub fn collect(project: &Project, known_rules: &BTreeSet<String>) -> Self {
        let mut set = Self::default();
        for file in &project.files {
            for (index, line) in file.text.lines().enumerate() {
                let Some((_, directive)) = line.split_once(MARKER) else {
                    continue;
                };
                if file.generated {
                    set.problem(
                        &file.rel_path,
                        index + 1,
                        "generated files cannot contain ultralint suppressions",
                        "change the generator or source contract and regenerate the file instead of suppressing generated output",
                    );
                    continue;
                }
                set.parse_directive(&file.rel_path, index + 1, directive, known_rules);
            }
        }
        set
    }

    pub fn suppresses(&mut self, rule_id: &str, path: &str, issue_line: Option<usize>) -> bool {
        let today = Utc::now().date_naive();
        self.entries.iter_mut().any(|entry| {
            let line_matches = issue_line
                .is_some_and(|line| line == entry.line || line == entry.line.saturating_add(1));
            let active = entry.expires.is_none_or(|expires| expires >= today);
            let matches = entry.rule_id == rule_id && entry.path == path && line_matches && active;
            if matches {
                entry.used = true;
            }
            matches
        })
    }

    fn parse_directive(
        &mut self,
        path: &str,
        line: usize,
        directive: &str,
        known_rules: &BTreeSet<String>,
    ) {
        let Some((rule_id, remainder)) = directive.trim().split_once(" -- ") else {
            self.problem(
                path,
                line,
                "suppression is missing a reason",
                "use `ultralint: allow <rule-id> -- <reason> until=YYYY-MM-DD`",
            );
            return;
        };
        let rule_id = rule_id.trim();
        if rule_id == "*" || rule_id.is_empty() {
            self.problem(
                path,
                line,
                "blanket ultralint suppression is not allowed",
                "name one exact rule and explain why the local exception is necessary",
            );
            return;
        }
        if !known_rules.contains(rule_id) {
            self.problem(
                path,
                line,
                format!("suppression references unknown rule `{rule_id}`"),
                "run `ultralint --list-rules` and use an exact current rule id",
            );
            return;
        }

        let mut reason_parts = Vec::new();
        let mut expires = None;
        for part in remainder.split_whitespace() {
            if let Some(value) = part.strip_prefix("until=") {
                match NaiveDate::parse_from_str(value, "%Y-%m-%d") {
                    Ok(date) => expires = Some(date),
                    Err(_) => {
                        self.problem(
                            path,
                            line,
                            format!("suppression has invalid expiry `{value}`"),
                            "use an ISO date such as `until=2026-12-31`",
                        );
                        return;
                    }
                }
            } else {
                reason_parts.push(part);
            }
        }
        if reason_parts.is_empty() {
            self.problem(
                path,
                line,
                "suppression reason is empty",
                "state the concrete technical reason for the exception",
            );
            return;
        }
        if let Some(date) = expires
            && date < Utc::now().date_naive()
        {
            self.problem(
                path,
                line,
                format!("suppression expired on {date}"),
                "remove the suppression or renew it with a current reason and expiry",
            );
        }
        self.entries.push(Suppression {
            path: path.to_string(),
            line,
            rule_id: rule_id.to_string(),
            reason: reason_parts.join(" "),
            expires,
            used: false,
        });
    }

    fn problem(
        &mut self,
        path: &str,
        line: usize,
        message: impl Into<String>,
        help: impl Into<String>,
    ) {
        self.diagnostics.push(SuppressionDiagnostic {
            path: path.to_string(),
            line,
            message: message.into(),
            help: help.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::SuppressionSet;

    #[test]
    fn rejects_blanket_and_reasonless_directives() {
        let known = BTreeSet::from(["example-rule".to_string()]);
        let mut set = SuppressionSet::default();
        set.parse_directive("src/a.ts", 1, "* -- forever", &known);
        set.parse_directive("src/a.ts", 2, "example-rule", &known);
        assert_eq!(set.diagnostics.len(), 2);
    }
}
