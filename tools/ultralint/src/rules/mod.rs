mod advisory_quality;
mod api_client_boundary;
mod api_error_boundary;
mod api_layer;
mod better_auth_imports;
mod boundaries;
mod cloudflare_ai_gateway;
mod cloudflare_worker_env;
mod common;
mod component_boundaries;
mod db_access_layer;
mod drizzle_rls_policies;
mod drizzle_schema_conventions;
mod generated_contract_sync;
mod generated_contracts;
mod hook_location;
mod migration_command_safety;
mod naming_complexity;
mod openapi_contracts;
mod origin_policy_safety;
mod permission_grammar;
mod quality_toolchain;
mod role_scoped_repositories;
mod route_thinness;
mod runtime_safety;
mod secrets;
mod server_function_policy;
mod server_layer_boundaries;
mod source_integrity;
mod sql_files;
mod structure;
mod suppression_hygiene;
mod tracked_secret_files;
mod types;
mod unique_project_symbols;
mod worker_db_connection_safety;
mod worker_event_contracts;
mod wrangler_environment;
mod wrangler_resource_isolation;
mod wrangler_schema;

use std::path::PathBuf;
use std::{collections::BTreeMap, collections::BTreeSet};

use crate::config::Config;
use crate::fs::Project;
use crate::suppression::SuppressionSet;

pub const POLICY_VERSION: &str = "2026-07-15";

pub trait Rule {
    fn id(&self) -> &'static str;
    fn category(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn check(&self, project: &Project, config: &Config, report: &mut Report);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    pub rule_id: &'static str,
    pub category: String,
    pub path: String,
    pub line: Option<usize>,
    pub message: String,
    pub help: Option<String>,
}

#[derive(Debug)]
pub struct Report {
    pub root: PathBuf,
    pub files_scanned: usize,
    pub issues: Vec<Issue>,
    rule_categories: BTreeMap<&'static str, &'static str>,
    suppressions: SuppressionSet,
}

impl Report {
    pub fn new(
        root: PathBuf,
        files_scanned: usize,
        rule_categories: BTreeMap<&'static str, &'static str>,
        suppressions: SuppressionSet,
    ) -> Self {
        Self {
            root,
            files_scanned,
            issues: Vec::new(),
            rule_categories,
            suppressions,
        }
    }

    pub fn error(
        &mut self,
        rule_id: &'static str,
        path: impl Into<String>,
        line: Option<usize>,
        message: impl Into<String>,
        help: impl Into<String>,
    ) {
        self.push(
            Severity::Error,
            rule_id,
            path.into(),
            line,
            message.into(),
            help.into(),
        );
    }

    pub fn warning(
        &mut self,
        rule_id: &'static str,
        path: impl Into<String>,
        line: Option<usize>,
        message: impl Into<String>,
        help: impl Into<String>,
    ) {
        self.push(
            Severity::Warning,
            rule_id,
            path.into(),
            line,
            message.into(),
            help.into(),
        );
    }

    fn push(
        &mut self,
        severity: Severity,
        rule_id: &'static str,
        path: String,
        line: Option<usize>,
        message: String,
        help: String,
    ) {
        if self.suppressions.suppresses(rule_id, &path, line) {
            return;
        }
        self.issues.push(Issue {
            severity,
            rule_id,
            category: self
                .rule_categories
                .get(rule_id)
                .copied()
                .unwrap_or("internal")
                .to_string(),
            path,
            line,
            message,
            help: if help.is_empty() { None } else { Some(help) },
        });
    }

    pub fn finalize(&mut self) {
        let diagnostics = std::mem::take(&mut self.suppressions.diagnostics);
        for diagnostic in diagnostics {
            self.push_unsuppressed(
                Severity::Error,
                "suppression-hygiene",
                diagnostic.path,
                Some(diagnostic.line),
                diagnostic.message,
                diagnostic.help,
            );
        }
        let unused = self
            .suppressions
            .entries
            .iter()
            .filter(|entry| !entry.used)
            .cloned()
            .collect::<Vec<_>>();
        for entry in unused {
            self.push_unsuppressed(
                Severity::Warning,
                "suppression-hygiene",
                entry.path,
                Some(entry.line),
                format!("unused suppression for `{}`", entry.rule_id),
                format!(
                    "remove the stale suppression; recorded reason was: {}",
                    entry.reason
                ),
            );
        }
        self.issues.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.rule_id.cmp(right.rule_id))
                .then_with(|| left.message.cmp(&right.message))
        });
    }

    fn push_unsuppressed(
        &mut self,
        severity: Severity,
        rule_id: &'static str,
        path: String,
        line: Option<usize>,
        message: String,
        help: String,
    ) {
        self.issues.push(Issue {
            severity,
            rule_id,
            category: self
                .rule_categories
                .get(rule_id)
                .copied()
                .unwrap_or("internal")
                .to_string(),
            path,
            line,
            message,
            help: (!help.is_empty()).then_some(help),
        });
    }

    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == Severity::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == Severity::Warning)
    }

    pub fn print_human(&self) {
        println!("ultralint: {}", self.root.display());
        println!("files scanned: {}", self.files_scanned);

        if self.issues.is_empty() {
            println!("no issues found");
            return;
        }

        for issue in &self.issues {
            let line = issue
                .line
                .map(|line| format!(":{line}"))
                .unwrap_or_default();
            println!(
                "[{}] {} {}{} - {}",
                issue.severity.as_str(),
                issue.rule_id,
                issue.path,
                line,
                issue.message
            );
            if let Some(help) = &issue.help {
                println!("  help: {help}");
            }
        }
    }

    pub fn to_json(&self) -> String {
        let errors = self
            .issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
            .count();
        let warnings = self.issues.len() - errors;
        serde_json::to_string_pretty(&serde_json::json!({
            "schemaVersion": 1,
            "toolVersion": env!("CARGO_PKG_VERSION"),
            "policyVersion": POLICY_VERSION,
            "root": self.root.display().to_string(),
            "filesScanned": self.files_scanned,
            "summary": {
                "errors": errors,
                "warnings": warnings,
                "total": self.issues.len(),
            },
            "issues": self.issues,
        }))
        .expect("ultralint report is serializable")
    }
}

pub fn builtin_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(structure::ProjectStructureRule),
        Box::new(generated_contracts::GeneratedContractsRule),
        Box::new(generated_contract_sync::GeneratedContractSyncRule),
        Box::new(openapi_contracts::OpenApiContractsRule),
        Box::new(api_client_boundary::ApiClientBoundaryRule),
        Box::new(api_error_boundary::ApiErrorBoundaryRule),
        Box::new(api_layer::ApiLayerRule),
        Box::new(runtime_safety::RuntimeSafetyRule),
        Box::new(source_integrity::SourceIntegrityRule),
        Box::new(suppression_hygiene::SuppressionHygieneRule),
        Box::new(server_function_policy::ServerFunctionPolicyRule),
        Box::new(server_layer_boundaries::ServerLayerBoundariesRule),
        Box::new(naming_complexity::NamingComplexityRule),
        Box::new(secrets::SecretBoundariesRule),
        Box::new(sql_files::SqlFilesRule),
        Box::new(types::TypeContractsRule),
        Box::new(boundaries::ArchitectureBoundariesRule),
        Box::new(component_boundaries::ComponentBoundariesRule),
        Box::new(cloudflare_worker_env::CloudflareWorkerEnvRule),
        Box::new(cloudflare_ai_gateway::CloudflareAiGatewayBindingRule),
        Box::new(better_auth_imports::BetterAuthImportsRule),
        Box::new(hook_location::HookLocationRule),
        Box::new(unique_project_symbols::UniqueProjectSymbolsRule),
        Box::new(route_thinness::RouteThinnessRule),
        Box::new(db_access_layer::DbAccessLayerRule),
        Box::new(drizzle_schema_conventions::DrizzleSchemaConventionsRule),
        Box::new(drizzle_rls_policies::DrizzleRlsPoliciesRule),
        Box::new(role_scoped_repositories::RoleScopedRepositoriesRule),
        Box::new(worker_db_connection_safety::WorkerDbConnectionSafetyRule),
        Box::new(migration_command_safety::MigrationCommandSafetyRule),
        Box::new(tracked_secret_files::TrackedSecretFilesRule),
        Box::new(origin_policy_safety::OriginPolicySafetyRule),
        Box::new(permission_grammar::PermissionGrammarRule),
        Box::new(wrangler_schema::WranglerSchemaRule),
        Box::new(wrangler_environment::WranglerEnvSurfaceRule),
        Box::new(wrangler_environment::EnvironmentFilesRule),
        Box::new(wrangler_resource_isolation::WranglerResourceIsolationRule),
        Box::new(worker_event_contracts::WorkerEventContractsRule),
        Box::new(quality_toolchain::QualityToolchainRule),
        Box::new(advisory_quality::FocusedTestsRule),
        Box::new(advisory_quality::TestSkipReasonsRule),
        Box::new(advisory_quality::DrizzleMutationSafetyRule),
        Box::new(advisory_quality::ImportCyclesRule),
        Box::new(advisory_quality::WorkerMutableStateRule),
        Box::new(advisory_quality::DeployWorkflowTargetsRule),
    ]
}

pub fn rule_categories(rules: &[Box<dyn Rule>]) -> BTreeMap<&'static str, &'static str> {
    rules
        .iter()
        .map(|rule| (rule.id(), rule.category()))
        .collect()
}

pub fn rule_ids(rules: &[Box<dyn Rule>]) -> BTreeSet<String> {
    rules.iter().map(|rule| rule.id().to_string()).collect()
}
