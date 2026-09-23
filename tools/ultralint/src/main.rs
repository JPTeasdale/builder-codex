mod analysis;
mod config;
mod fs;
mod rules;
mod structured;
mod suppression;

use std::env;
use std::path::PathBuf;
use std::process;

use config::Config;
use fs::Project;
use rules::{POLICY_VERSION, Report, builtin_rules, rule_categories, rule_ids};
use suppression::SuppressionSet;

#[derive(Debug)]
struct Cli {
    root: PathBuf,
    json: bool,
    list_rules: bool,
    deny_warnings: bool,
    explain_config: bool,
    version: bool,
}

impl Cli {
    fn parse() -> Result<Self, String> {
        let mut root: Option<PathBuf> = None;
        let mut json = false;
        let mut list_rules = false;
        let mut deny_warnings = false;
        let mut explain_config = false;
        let mut version = false;

        for arg in env::args().skip(1) {
            if arg == "--json" {
                json = true;
            } else if arg == "--list-rules" {
                list_rules = true;
            } else if arg == "--deny-warnings" {
                deny_warnings = true;
            } else if arg == "--explain-config" {
                explain_config = true;
            } else if arg == "-V" || arg == "--version" {
                version = true;
            } else if arg == "-h" || arg == "--help" {
                print_help();
                process::exit(0);
            } else if arg.starts_with('-') {
                return Err(format!("unknown option: {arg}"));
            } else if root.is_none() {
                root = Some(PathBuf::from(arg));
            } else {
                return Err(format!("unexpected positional argument: {arg}"));
            }
        }

        Ok(Self {
            root: root.unwrap_or_else(|| PathBuf::from(".")),
            json,
            list_rules,
            deny_warnings,
            explain_config,
            version,
        })
    }
}

fn print_help() {
    println!(
        "ultralint\n\nUsage:\n  ultralint [project-root] [options]\n\nOptions:\n  --json             Emit the versioned JSON report schema\n  --list-rules       List built-in rules\n  --explain-config   Show detected app slots and policy inputs\n  --deny-warnings    Exit 1 when warnings are present\n  -V, --version      Print tool and policy versions\n  -h, --help         Show this help\n\nExit codes:\n  0  no errors (and no warnings with --deny-warnings)\n  1  policy findings\n  2  CLI, filesystem, or analyzer failure\n\nPolicy:\n  Opinionated built-in enforcement for React webapps running on Cloudflare Workers. No project-local configuration is supported."
    );
}

fn main() {
    let json_requested = env::args().any(|arg| arg == "--json");
    let cli = match Cli::parse() {
        Ok(cli) => cli,
        Err(err) => {
            fatal(
                json_requested,
                format!("{err}. Run ultralint --help for usage."),
            );
        }
    };

    if cli.version {
        println!(
            "ultralint {} (policy {})",
            env!("CARGO_PKG_VERSION"),
            POLICY_VERSION
        );
        return;
    }

    let root = match cli.root.canonicalize() {
        Ok(root) => root,
        Err(err) => {
            fatal(
                cli.json,
                format!("cannot read project root {:?}: {err}", cli.root),
            );
        }
    };

    let config = Config::load(&root);

    let rules = builtin_rules();
    if cli.list_rules {
        if cli.json {
            let entries = rules
                .iter()
                .map(|rule| {
                    serde_json::json!({
                        "id": rule.id(),
                        "category": rule.category(),
                        "description": rule.description(),
                    })
                })
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&entries).expect("rule catalog is serializable")
            );
        } else {
            for rule in &rules {
                println!("{}\t{}\t{}", rule.id(), rule.category(), rule.description());
            }
        }
        return;
    }

    if cli.explain_config {
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&config).expect("config is serializable")
            );
        } else {
            println!("{config:#?}");
        }
        return;
    }

    let project = match Project::load(root.clone()) {
        Ok(project) => project,
        Err(err) => {
            fatal(cli.json, format!("failed to load project: {err}"));
        }
    };

    let categories = rule_categories(&rules);
    let known_rules = rule_ids(&rules);
    let suppressions = SuppressionSet::collect(&project, &known_rules);
    let mut report = Report::new(root, project.files.len(), categories, suppressions);
    for rule in &rules {
        rule.check(&project, &config, &mut report);
    }
    report.finalize();

    if cli.json {
        println!("{}", report.to_json());
    } else {
        report.print_human();
    }

    if report.has_errors() || (cli.deny_warnings && report.has_warnings()) {
        process::exit(1);
    }
}

fn fatal(json: bool, message: String) -> ! {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "toolVersion": env!("CARGO_PKG_VERSION"),
                "policyVersion": POLICY_VERSION,
                "failure": {
                    "kind": "operational",
                    "message": message,
                },
            }))
            .expect("operational failure is serializable")
        );
    } else {
        eprintln!("ultralint: {message}");
    }
    process::exit(2);
}
