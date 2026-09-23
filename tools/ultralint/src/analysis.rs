use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, ExportAllDeclaration, ExportNamedDeclaration, ImportDeclaration,
    ImportDeclarationSpecifier, ImportExpression, ImportOrExportKind, StringLiteral,
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
pub struct CallFact {
    pub callee: String,
    pub arguments: Vec<String>,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct StringFact {
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct TsAnalysis {
    pub imports: Vec<ImportFact>,
    pub calls: Vec<CallFact>,
    pub strings: Vec<StringFact>,
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
    fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
        self.analysis.strings.push(StringFact {
            value: literal.value.to_string(),
            line: self.line(literal.span.start),
        });
    }

    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
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
        });
        walk::walk_call_expression(self, call);
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
}
