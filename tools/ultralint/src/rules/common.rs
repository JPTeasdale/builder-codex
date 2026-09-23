use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationKind {
    Function,
    Const,
    Let,
    Var,
    Class,
    Interface,
    Type,
    Enum,
}

impl DeclarationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Const => "const",
            Self::Let => "let",
            Self::Var => "var",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::Enum => "enum",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Declaration {
    pub kind: DeclarationKind,
    pub name: String,
    pub exported: bool,
    pub file: String,
    pub line: usize,
}

pub fn parse_top_level_declaration(
    line: &str,
    file: &str,
    line_number: usize,
) -> Option<Declaration> {
    let trimmed = line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*")
    {
        return None;
    }

    if line.len() != trimmed.len() {
        return None;
    }

    let mut rest = trimmed;
    let mut exported = false;
    if let Some(next) = rest.strip_prefix("export ") {
        exported = true;
        rest = next.trim_start();
    }
    if let Some(next) = rest.strip_prefix("default ") {
        exported = true;
        rest = next.trim_start();
    }
    if let Some(next) = rest.strip_prefix("declare ") {
        rest = next.trim_start();
    }
    if let Some(next) = rest.strip_prefix("abstract ") {
        rest = next.trim_start();
    }
    if let Some(next) = rest.strip_prefix("async ") {
        rest = next.trim_start();
    }

    for (prefix, kind) in [
        ("function ", DeclarationKind::Function),
        ("const ", DeclarationKind::Const),
        ("let ", DeclarationKind::Let),
        ("var ", DeclarationKind::Var),
        ("class ", DeclarationKind::Class),
        ("interface ", DeclarationKind::Interface),
        ("type ", DeclarationKind::Type),
        ("enum ", DeclarationKind::Enum),
    ] {
        let Some(name_rest) = rest.strip_prefix(prefix) else {
            continue;
        };
        let name = parse_identifier(name_rest);
        if !name.is_empty() {
            return Some(Declaration {
                kind,
                name,
                exported,
                file: file.to_string(),
                line: line_number,
            });
        }
    }

    None
}

pub fn is_ts_source_file(rel_path: &str) -> bool {
    (rel_path.ends_with(".ts") || rel_path.ends_with(".tsx")) && !rel_path.ends_with(".d.ts")
}

pub fn is_test_file(rel_path: &str) -> bool {
    rel_path.contains("__tests__/")
        || rel_path.contains("/tests/")
        || rel_path.ends_with(".test.ts")
        || rel_path.ends_with(".test.tsx")
        || rel_path.ends_with(".spec.ts")
        || rel_path.ends_with(".spec.tsx")
}

pub fn is_route_file(rel_path: &str, config: &Config) -> bool {
    is_ts_source_file(rel_path)
        && config
            .web_apps
            .iter()
            .any(|root| has_prefix(rel_path, root, "src/routes/"))
}

pub fn has_prefix(rel_path: &str, root: &str, suffix: &str) -> bool {
    if root == "." || root.is_empty() {
        rel_path.starts_with(suffix)
    } else {
        rel_path.starts_with(&format!("{}/{}", root.trim_end_matches('/'), suffix))
    }
}

pub fn join(root: &str, rel_path: &str) -> String {
    if root == "." || root.is_empty() {
        rel_path.to_string()
    } else {
        format!("{}/{}", root.trim_end_matches('/'), rel_path)
    }
}

pub fn has_path_segment(rel_path: &str, segment: &str) -> bool {
    rel_path.split('/').any(|part| part == segment)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectProperty<'a> {
    pub name: String,
    pub value: &'a str,
}

pub fn object_properties(text: &str) -> Option<Vec<ObjectProperty<'_>>> {
    let start = text.find(|ch: char| !ch.is_whitespace())?;
    if text.as_bytes().get(start) != Some(&b'{') {
        return None;
    }
    let end = matching_delimiter(text, start)?;
    if !text[end + 1..].trim().is_empty() {
        return None;
    }

    let mut properties = Vec::new();
    let mut cursor = start + 1;
    while cursor < end {
        cursor = skip_trivia_and_separators(text, cursor, end);
        if cursor >= end {
            break;
        }

        if text[cursor..].starts_with("...") {
            cursor = scan_property_value(text, cursor + 3, end);
            continue;
        }

        let (name, next) = parse_property_name(text, cursor, end)?;
        cursor = skip_trivia(text, next, end);
        if text.as_bytes().get(cursor) == Some(&b'?') {
            cursor = skip_trivia(text, cursor + 1, end);
        }
        if text.as_bytes().get(cursor) != Some(&b':') {
            cursor = scan_property_value(text, cursor, end);
            continue;
        }

        let value_start = skip_trivia(text, cursor + 1, end);
        let value_end = scan_property_value(text, value_start, end);
        properties.push(ObjectProperty {
            name,
            value: text[value_start..value_end].trim(),
        });
        cursor = value_end;
    }

    Some(properties)
}

pub fn string_literal(text: &str) -> Option<String> {
    let text = text.trim();
    let quote = *text.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"' | b'`') || text.as_bytes().last() != Some(&quote) {
        return None;
    }
    if quote == b'`' && text.contains("${") {
        return None;
    }

    let mut value = String::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            value.push(ch);
            continue;
        }
        let escaped = chars.next()?;
        value.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'b' => '\u{0008}',
            'f' => '\u{000c}',
            other => other,
        });
    }
    Some(value)
}

pub fn matching_delimiter(text: &str, open: usize) -> Option<usize> {
    let opening = *text.as_bytes().get(open)?;
    let closing = match opening {
        b'{' => b'}',
        b'[' => b']',
        b'(' => b')',
        _ => return None,
    };
    let mut depth = 1usize;
    let mut cursor = open + 1;
    while cursor < text.len() {
        match text.as_bytes()[cursor] {
            b'\'' | b'"' | b'`' => cursor = quoted_end(text, cursor)?,
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'/') => {
                cursor = line_comment_end(text, cursor + 2)
            }
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'*') => {
                cursor = block_comment_end(text, cursor + 2)?
            }
            byte if byte == opening => {
                depth += 1;
                cursor += 1;
            }
            byte if byte == closing => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }
    None
}

pub fn line_number_at(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

pub fn code_only(text: &str) -> String {
    let mut output = text.as_bytes().to_vec();
    let mut cursor = 0usize;
    while cursor < text.len() {
        let end = match text.as_bytes()[cursor] {
            b'\'' | b'"' | b'`' => quoted_end(text, cursor),
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'/') => {
                Some(line_comment_end(text, cursor + 2))
            }
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'*') => {
                block_comment_end(text, cursor + 2)
            }
            _ => None,
        };
        let Some(end) = end else {
            cursor += 1;
            continue;
        };
        for byte in &mut output[cursor..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        cursor = end;
    }
    String::from_utf8(output).expect("masking ASCII syntax preserves UTF-8")
}

pub fn named_object_blocks<'a>(text: &'a str, keyword: &str, name: &str) -> Vec<&'a str> {
    let code = code_only(text);
    let mut blocks = Vec::new();
    for (offset, _) in code.match_indices(keyword) {
        if !has_identifier_boundaries(&code, offset, keyword.len()) {
            continue;
        }
        let mut cursor = offset + keyword.len();
        while code
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if code.as_bytes().get(cursor..cursor + name.len()) != Some(name.as_bytes())
            || !has_identifier_boundaries(&code, cursor, name.len())
        {
            continue;
        }
        let Some(relative_open) = code[cursor + name.len()..].find('{') else {
            continue;
        };
        let open = cursor + name.len() + relative_open;
        if code[cursor + name.len()..open].contains(';') {
            continue;
        }
        let Some(close) = matching_delimiter(text, open) else {
            continue;
        };
        blocks.push(&text[open..=close]);
    }
    blocks
}

fn has_identifier_boundaries(text: &str, offset: usize, len: usize) -> bool {
    let before = offset
        .checked_sub(1)
        .and_then(|index| text.as_bytes().get(index));
    let after = text.as_bytes().get(offset + len);
    !before.is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        && !after.is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

fn parse_property_name(text: &str, cursor: usize, end: usize) -> Option<(String, usize)> {
    match *text.as_bytes().get(cursor)? {
        b'\'' | b'"' | b'`' => {
            let next = quoted_end(text, cursor)?;
            Some((string_literal(&text[cursor..next])?, next))
        }
        b'[' => {
            let close = matching_delimiter(text, cursor)?;
            if close >= end {
                return None;
            }
            let name = string_literal(&text[cursor + 1..close])?;
            Some((name, close + 1))
        }
        _ => {
            let mut next = cursor;
            while next < end {
                let byte = text.as_bytes()[next];
                if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'-') {
                    next += 1;
                } else {
                    break;
                }
            }
            (next > cursor).then(|| (text[cursor..next].to_string(), next))
        }
    }
}

fn scan_property_value(text: &str, mut cursor: usize, end: usize) -> usize {
    let mut delimiters = Vec::new();
    while cursor < end {
        match text.as_bytes()[cursor] {
            b'\'' | b'"' | b'`' => {
                cursor = quoted_end(text, cursor).unwrap_or(end);
            }
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'/') => {
                cursor = line_comment_end(text, cursor + 2);
            }
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'*') => {
                cursor = block_comment_end(text, cursor + 2).unwrap_or(end);
            }
            b'{' => {
                delimiters.push(b'}');
                cursor += 1;
            }
            b'[' => {
                delimiters.push(b']');
                cursor += 1;
            }
            b'(' => {
                delimiters.push(b')');
                cursor += 1;
            }
            byte if delimiters.last() == Some(&byte) => {
                delimiters.pop();
                cursor += 1;
            }
            b',' | b';' if delimiters.is_empty() => break,
            _ => cursor += 1,
        }
    }
    cursor
}

fn skip_trivia_and_separators(text: &str, mut cursor: usize, end: usize) -> usize {
    loop {
        cursor = skip_trivia(text, cursor, end);
        if cursor < end && matches!(text.as_bytes()[cursor], b',' | b';') {
            cursor += 1;
        } else {
            return cursor;
        }
    }
}

fn skip_trivia(text: &str, mut cursor: usize, end: usize) -> usize {
    while cursor < end {
        match text.as_bytes()[cursor] {
            byte if byte.is_ascii_whitespace() => cursor += 1,
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'/') => {
                cursor = line_comment_end(text, cursor + 2).min(end)
            }
            b'/' if text.as_bytes().get(cursor + 1) == Some(&b'*') => {
                cursor = block_comment_end(text, cursor + 2).unwrap_or(end).min(end)
            }
            _ => break,
        }
    }
    cursor
}

fn quoted_end(text: &str, start: usize) -> Option<usize> {
    let quote = *text.as_bytes().get(start)?;
    let mut cursor = start + 1;
    let mut escaped = false;
    while cursor < text.len() {
        let byte = text.as_bytes()[cursor];
        cursor += 1;
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == quote {
            return Some(cursor);
        }
    }
    None
}

fn line_comment_end(text: &str, start: usize) -> usize {
    text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset + 1)
}

fn block_comment_end(text: &str, start: usize) -> Option<usize> {
    text[start..].find("*/").map(|offset| start + offset + 2)
}

fn parse_identifier(value: &str) -> String {
    value
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$')
        .collect()
}

#[cfg(test)]
mod structured_tests {
    use super::{code_only, named_object_blocks, object_properties, string_literal};

    #[test]
    fn parses_nested_object_properties_without_splitting_nested_values() {
        let properties = object_properties(
            r#"{ method: 'get', responses: { 200: { schema: z.object({ ok: z.boolean() }) } } }"#,
        )
        .expect("object should parse");

        assert_eq!(properties.len(), 2);
        assert_eq!(properties[0].name, "method");
        assert_eq!(string_literal(properties[0].value).as_deref(), Some("get"));
        assert_eq!(properties[1].name, "responses");
        assert!(properties[1].value.contains("z.object"));
    }

    #[test]
    fn masks_comments_and_strings_without_changing_lines() {
        let masked = code_only("// fetch('/api')\nconst text = \"error.stack\";\nfetch(url);");
        assert_eq!(masked.lines().count(), 3);
        assert!(!masked.contains("/api"));
        assert!(masked.contains("fetch(url)"));
    }

    #[test]
    fn finds_named_interface_blocks_outside_comments() {
        let blocks = named_object_blocks(
            "// interface Env { FAKE: string }\ninterface Env extends Base { REAL: string; }",
            "interface",
            "Env",
        );
        assert_eq!(blocks, ["{ REAL: string; }"]);
    }
}
