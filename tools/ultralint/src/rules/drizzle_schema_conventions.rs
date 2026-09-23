use crate::config::Config;
use crate::fs::Project;
use crate::rules::common::{has_prefix, is_ts_source_file, join};
use crate::rules::{Report, Rule};

pub struct DrizzleSchemaConventionsRule;

impl Rule for DrizzleSchemaConventionsRule {
    fn id(&self) -> &'static str {
        "drizzle-schema-conventions"
    }

    fn category(&self) -> &'static str {
        "database"
    }

    fn description(&self) -> &'static str {
        "standardizes Drizzle schema keys, timestamps, and shared helper locations"
    }

    fn check(&self, project: &Project, config: &Config, report: &mut Report) {
        for file in &project.files {
            if !is_ts_source_file(&file.rel_path) {
                continue;
            }

            if is_legacy_lib_schema_path(&file.rel_path, config) {
                report.error(
                    self.id(),
                    &file.rel_path,
                    None,
                    "src/lib/db/schema is not allowed",
                    "move Drizzle schema to src/server/db/schema; database schema is server-owned.",
                );
                continue;
            }

            if is_generated_schema_file(&file.rel_path) {
                continue;
            }

            let Some(schema_root) = server_schema_root_for(&file.rel_path, config) else {
                continue;
            };

            let keys_path = join(&schema_root, "shared/keys.ts");
            let timestamps_path = join(&schema_root, "shared/timestamps.ts");
            let is_keys_helper = file.rel_path == keys_path;
            let is_timestamps_helper = file.rel_path == timestamps_path;

            if is_keys_helper {
                check_key_helper_shape(project, &keys_path, report, self.id());
            }
            if is_timestamps_helper {
                check_timestamp_helper_shape(project, &timestamps_path, report, self.id());
            }

            for (index, line) in file.text.lines().enumerate() {
                let line_number = index + 1;
                if is_comment_line(line) {
                    continue;
                }

                if contains_call(line, "timestamp") && !is_timestamps_helper {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "drizzle timestamp column must use timestampOptimized()",
                        timestamp_usage_help(),
                    );
                }

                if contains_direct_id_column(line) && !is_keys_helper {
                    report.error(
                        self.id(),
                        &file.rel_path,
                        Some(line_number),
                        "drizzle id column must use keyId()",
                        key_usage_help(),
                    );
                }
            }
        }

        for root in &config.web_apps {
            let schema_root = join(root, "src/server/db/schema");
            if !schema_root_has_handwritten_tables(project, &schema_root) {
                continue;
            }

            let keys_path = join(&schema_root, "shared/keys.ts");
            if !project.exists(&keys_path) {
                report.error(
                    self.id(),
                    keys_path,
                    None,
                    "drizzle schema is missing shared key helpers",
                    key_helper_help(),
                );
            }

            let timestamps_path = join(&schema_root, "shared/timestamps.ts");
            if !project.exists(&timestamps_path) {
                report.error(
                    self.id(),
                    timestamps_path,
                    None,
                    "drizzle schema is missing shared timestamp helpers",
                    timestamp_helper_help(),
                );
            }
        }
    }
}

fn is_legacy_lib_schema_path(rel_path: &str, config: &Config) -> bool {
    config
        .web_apps
        .iter()
        .any(|root| has_prefix(rel_path, root, "src/lib/db/schema/"))
        || rel_path.contains("/src/lib/db/schema/")
}

fn server_schema_root_for(rel_path: &str, config: &Config) -> Option<String> {
    for root in &config.web_apps {
        let schema_root = join(root, "src/server/db/schema");
        if rel_path.starts_with(&format!("{schema_root}/")) {
            return Some(schema_root);
        }
    }
    None
}

fn schema_root_has_handwritten_tables(project: &Project, schema_root: &str) -> bool {
    let prefix = format!("{schema_root}/");
    project.files.iter().any(|file| {
        is_ts_source_file(&file.rel_path)
            && file.rel_path.starts_with(&prefix)
            && !is_generated_schema_file(&file.rel_path)
            && file.text.contains("pgTable(")
    })
}

fn is_generated_schema_file(rel_path: &str) -> bool {
    rel_path.ends_with(".gen.ts") || rel_path.ends_with(".gen.d.ts")
}

fn check_key_helper_shape(
    project: &Project,
    rel_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(text) = project.read(rel_path) else {
        return;
    };
    if normalized(text).contains(KEY_HELPER_NORMALIZED) {
        return;
    }
    report.error(
        rule_id,
        rel_path,
        None,
        "keyId() does not match the required identity key helper",
        key_helper_help(),
    );
}

fn check_timestamp_helper_shape(
    project: &Project,
    rel_path: &str,
    report: &mut Report,
    rule_id: &'static str,
) {
    let Some(text) = project.read(rel_path) else {
        return;
    };
    if normalized(text).contains(TIMESTAMP_HELPER_NORMALIZED) {
        return;
    }
    report.error(
        rule_id,
        rel_path,
        None,
        "timestampOptimized() does not match the required timestamp helper",
        timestamp_helper_help(),
    );
}

fn contains_call(line: &str, name: &str) -> bool {
    let Some(start) = line.find(name) else {
        return false;
    };
    let before = line[..start].chars().next_back();
    let after = line[start + name.len()..].chars().next();
    let before_ok = before.is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'));
    before_ok && after == Some('(')
}

fn contains_direct_id_column(line: &str) -> bool {
    let compact = line.replace(' ', "");
    compact.contains("id:") && (compact.contains("(\"id\")") || compact.contains("('id')"))
}

fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*")
}

fn normalized(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn key_helper_help() -> &'static str {
    "Use this exact helper in src/server/db/schema/shared/keys.ts:\n\nimport { integer } from \"drizzle-orm/pg-core\";\n\nexport function keyId<T extends number = number>() {\n\treturn {\n\t\tid: integer(\"id\").primaryKey().generatedAlwaysAsIdentity().$type<T>(),\n\t};\n}"
}

fn timestamp_helper_help() -> &'static str {
    "Use this exact helper in src/server/db/schema/shared/timestamps.ts:\n\nimport { timestamp } from \"drizzle-orm/pg-core\";\n\nexport function timestampOptimized(name: string) {\n\treturn timestamp(name, {\n\t\tmode: \"date\",\n\t\tprecision: 3,\n\t\twithTimezone: true,\n\t});\n}"
}

fn key_usage_help() -> &'static str {
    "Replace direct id columns like id: integer(\"id\"), id: text(\"id\"), or id: uuid(\"id\") with ...keyId(). Define the helper as:\n\nexport function keyId<T extends number = number>() {\n\treturn {\n\t\tid: integer(\"id\").primaryKey().generatedAlwaysAsIdentity().$type<T>(),\n\t};\n}"
}

fn timestamp_usage_help() -> &'static str {
    "Replace direct timestamp(...) calls with timestampOptimized(...). Define the helper as:\n\nexport function timestampOptimized(name: string) {\n\treturn timestamp(name, {\n\t\tmode: \"date\",\n\t\tprecision: 3,\n\t\twithTimezone: true,\n\t});\n}"
}

const KEY_HELPER_NORMALIZED: &str = "exportfunctionkeyId<Textendsnumber=number>(){return{id:integer(\"id\").primaryKey().generatedAlwaysAsIdentity().$type<T>(),};}";

const TIMESTAMP_HELPER_NORMALIZED: &str = "exportfunctiontimestampOptimized(name:string){returntimestamp(name,{mode:\"date\",precision:3,withTimezone:true,});}";
