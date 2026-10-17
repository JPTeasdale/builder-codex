use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, ChainElement, ExportAllDeclaration, ExportNamedDeclaration,
    Expression, ImportDeclaration, ImportDeclarationSpecifier, ImportExpression,
    ImportOrExportKind, JSXOpeningElement, StringLiteral,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Static,
    ReExport,
    Dynamic,
    Require,
}

#[derive(Debug, Clone)]
pub struct ImportFact {
    pub source: String,
    pub kind: ImportKind,
    pub type_only: bool,
    pub line: usize,
    pub local_names: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InlineTypeImportFact {
    pub source: String,
    pub specifiers: Vec<String>,
    pub line: usize,
    pub specifier_lines: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct CallFact {
    pub callee: String,
    pub arguments: Vec<String>,
    pub text: String,
    pub line: usize,
    pub start: u32,
    pub end: u32,
    pub receiver: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
pub struct StringFact {
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct JsxElementFact {
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TsAnalysis {
    pub imports: Vec<ImportFact>,
    pub inline_type_imports: Vec<InlineTypeImportFact>,
    pub calls: Vec<CallFact>,
    pub strings: Vec<StringFact>,
    pub jsx_elements: Vec<JsxElementFact>,
    pub parse_errors: Vec<String>,
}

impl TsAnalysis {
    pub fn imports_from(&self, source: &str) -> bool {
        self.imports.iter().any(|fact| fact.source == source)
    }
}

pub fn analyze_typescript(rel_path: &str, source: &str) -> TsAnalysis {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(rel_path).unwrap_or_else(|_| SourceType::ts());
    let parsed = Parser::new(&allocator, source, source_type).parse();

    let mut analysis = TsAnalysis {
        parse_errors: parsed.diagnostics.iter().map(ToString::to_string).collect(),
        ..TsAnalysis::default()
    };
    let mut visitor = FactVisitor::new(source, &mut analysis);
    visitor.visit_program(&parsed.program);
    analysis.imports.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.source.cmp(&right.source))
    });
    analysis.inline_type_imports.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.source.cmp(&right.source))
    });
    analysis.calls.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.callee.cmp(&right.callee))
    });
    analysis.strings.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.value.cmp(&right.value))
    });
    analysis.jsx_elements.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.name.cmp(&right.name))
    });
    analysis
}

struct FactVisitor<'source, 'analysis> {
    source: &'source str,
    line_starts: Vec<usize>,
    analysis: &'analysis mut TsAnalysis,
}

impl<'source, 'analysis> FactVisitor<'source, 'analysis> {
    fn new(source: &'source str, analysis: &'analysis mut TsAnalysis) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            source,
            line_starts,
            analysis,
        }
    }

    fn text(&self, span: Span) -> String {
        self.source
            .get(span.start as usize..span.end as usize)
            .unwrap_or_default()
            .to_string()
    }

    fn line(&self, offset: u32) -> usize {
        match self.line_starts.binary_search(&(offset as usize)) {
            Ok(index) => index + 1,
            Err(index) => index.max(1),
        }
    }

    fn push_import(
        &mut self,
        source: &str,
        kind: ImportKind,
        type_only: bool,
        span: Span,
        local_names: Vec<String>,
    ) {
        self.analysis.imports.push(ImportFact {
            source: source.to_string(),
            kind,
            type_only,
            line: self.line(span.start),
            local_names,
        });
    }
}

impl<'a> Visit<'a> for FactVisitor<'_, '_> {
    fn visit_jsx_opening_element(&mut self, element: &JSXOpeningElement<'a>) {
        if let Some(name) = element.name.get_identifier_name() {
            self.analysis.jsx_elements.push(JsxElementFact {
                name: name.to_string(),
                line: self.line(element.span.start),
            });
        }
        walk::walk_jsx_opening_element(self, element);
    }

    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        self.analysis.strings.push(StringFact {
            value: literal.value.to_string(),
            line: self.line(literal.span.start),
        });
    }

    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
        if declaration.import_kind != ImportOrExportKind::Type {
            let mut specifiers = Vec::new();
            let mut specifier_lines = Vec::new();
            for specifier in declaration.specifiers.iter().flatten() {
                let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                    continue;
                };
                if specifier.import_kind != ImportOrExportKind::Type {
                    continue;
                }
                let text = self.text(specifier.span);
                specifiers.push(
                    text.trim_start()
                        .strip_prefix("type ")
                        .unwrap_or(text.trim())
                        .trim()
                        .to_string(),
                );
                specifier_lines.push(self.line(specifier.span.start));
            }
            if !specifiers.is_empty() {
                self.analysis
                    .inline_type_imports
                    .push(InlineTypeImportFact {
                        source: declaration.source.value.to_string(),
                        specifiers,
                        line: self.line(declaration.span.start),
                        specifier_lines,
                    });
            }
        }

        let local_names = declaration
            .specifiers
            .iter()
            .flatten()
            .map(|specifier| match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                    specifier.local.name.to_string()
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                    specifier.local.name.to_string()
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                    specifier.local.name.to_string()
                }
            })
            .collect();
        self.push_import(
            declaration.source.value.as_str(),
            ImportKind::Static,
            declaration.import_kind == ImportOrExportKind::Type,
            declaration.span,
            local_names,
        );
        walk::walk_import_declaration(self, declaration);
    }

    fn visit_export_named_declaration(&mut self, declaration: &ExportNamedDeclaration<'a>) {
        if let Some(source) = &declaration.source {
            self.push_import(
                source.value.as_str(),
                ImportKind::ReExport,
                declaration.export_kind == ImportOrExportKind::Type,
                declaration.span,
                Vec::new(),
            );
        }
        walk::walk_export_named_declaration(self, declaration);
    }

    fn visit_export_all_declaration(&mut self, declaration: &ExportAllDeclaration<'a>) {
        self.push_import(
            declaration.source.value.as_str(),
            ImportKind::ReExport,
            false,
            declaration.span,
            Vec::new(),
        );
        walk::walk_export_all_declaration(self, declaration);
    }

    fn visit_import_expression(&mut self, expression: &ImportExpression<'a>) {
        let source = self.text(expression.source.span());
        let source = unquote(&source);
        if !source.is_empty() {
            self.push_import(
                source,
                ImportKind::Dynamic,
                false,
                expression.span,
                Vec::new(),
            );
        }
        walk::walk_import_expression(self, expression);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        let callee = self.text(call.callee.span());
        let receiver = receiver_call_span(call).map(|span| (span.start, span.end));
        let arguments = call
            .arguments
            .iter()
            .map(|argument| match argument {
                Argument::SpreadElement(spread) => self.text(spread.span),
                _ => self.text(argument.span()),
            })
            .collect::<Vec<_>>();
        if callee == "require"
            && let Some(source) = arguments.first().map(|value| unquote(value))
            && !source.is_empty()
        {
            self.push_import(source, ImportKind::Require, false, call.span, Vec::new());
        }
        self.analysis.calls.push(CallFact {
            callee,
            arguments,
            text: self.text(call.span),
            line: self.line(call.span.start),
            start: call.span.start,
            end: call.span.end,
            receiver,
        });
        walk::walk_call_expression(self, call);
    }
}

fn receiver_call_span(call: &CallExpression<'_>) -> Option<Span> {
    let member = call.callee.get_member_expr()?;
    expression_call_span(member.object())
}

fn expression_call_span(expression: &Expression<'_>) -> Option<Span> {
    match expression.get_inner_expression() {
        Expression::CallExpression(call) => Some(call.span),
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => Some(call.span),
            ChainElement::TSNonNullExpression(non_null) => {
                expression_call_span(&non_null.expression)
            }
            _ => None,
        },
        _ => None,
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::{ImportKind, analyze_typescript};

    #[test]
    fn extracts_multiline_imports_reexports_and_calls() {
        let facts = analyze_typescript(
            "src/example.ts",
            r#"
                import {
                    value,
                } from "~/server/domain";
                export * from "./other";
                const lazy = import("./lazy");
                fetch(
                    "/api/v1/example",
                );
            "#,
        );

        assert!(facts.parse_errors.is_empty());
        assert!(
            facts.imports.iter().any(|fact| {
                fact.source == "~/server/domain" && fact.kind == ImportKind::Static
            })
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|fact| fact.source == "./other" && fact.kind == ImportKind::ReExport)
        );
        assert!(
            facts
                .imports
                .iter()
                .any(|fact| fact.source == "./lazy" && fact.kind == ImportKind::Dynamic)
        );
        assert!(
            facts
                .calls
                .iter()
                .any(|fact| fact.callee == "fetch" && fact.arguments[0].contains("/api/v1"))
        );
    }

    #[test]
    fn extracts_jsx_opening_elements_without_comments_strings_or_closing_tags() {
        let facts = analyze_typescript(
            "src/example.tsx",
            r#"
                // <button>not JSX</button>
                const example = "<select>not JSX</select>";
                export function Example() {
                    return <form><button /><Select /><x-button /></form>;
                }
            "#,
        );

        assert!(facts.parse_errors.is_empty());
        let mut names = facts
            .jsx_elements
            .iter()
            .map(|element| element.name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(names, ["Select", "button", "form", "x-button"]);
    }

    #[test]
    fn extracts_inline_type_imports_from_mixed_multiline_imports() {
        let facts = analyze_typescript(
            "src/example.tsx",
            r#"
                import {
                    type Credential,
                    credentialLabels,
                    type Location as WorkLocation,
                } from "~/lib/application/application.schema";
                import type { Employee, Shift } from "~/lib/staffing/staffing.schema";
            "#,
        );

        assert!(facts.parse_errors.is_empty());
        assert_eq!(facts.inline_type_imports.len(), 1);
        let import = &facts.inline_type_imports[0];
        assert_eq!(import.source, "~/lib/application/application.schema");
        assert_eq!(
            import.specifiers,
            ["Credential", "Location as WorkLocation"]
        );
        assert_eq!(import.specifier_lines, [3, 5]);
    }
}
