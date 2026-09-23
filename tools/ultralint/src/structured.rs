use serde_json::Value;

pub fn parse_jsonc(text: &str) -> Result<Value, String> {
    jsonc_parser::parse_to_serde_value(text, &Default::default()).map_err(|error| error.to_string())
}

pub fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}

pub fn parse_yaml(text: &str) -> Result<Value, String> {
    serde_yaml_ng::from_str(text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_jsonc;

    #[test]
    fn jsonc_comments_cannot_join_tokens() {
        assert!(parse_jsonc(r#"{"value": 1/* comment */2}"#).is_err());
        assert!(parse_jsonc(r#"{"value": 1 /* unterminated"#).is_err());
    }
}
