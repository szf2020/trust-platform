//! Hover information for Structured Text.
//!
//! This module provides hover functionality to display type information
//! and documentation when hovering over symbols.

use text_size::{TextRange, TextSize};

use smol_str::SmolStr;
use trust_hir::db::SemanticDatabase;
use trust_hir::diagnostics::DiagnosticCode;
use trust_hir::symbols::{ScopeId, SymbolModifiers, SymbolTable, VarQualifier, Visibility};
use trust_hir::{Database, SourceDatabase, Symbol, SymbolKind, Type, TypeId};
use trust_syntax::parser::parse;
use trust_syntax::syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};
use trust_syntax::{lex, TokenKind};

use crate::signature_help::signature_help;
use crate::stdlib_docs::{self, StdlibFilter};
use crate::util::{
    field_type, ident_at_offset, ident_token_in_name, name_range_from_node,
    namespace_path_for_symbol, scope_at_position, using_path_for_symbol, IdeContext,
    ResolvedTarget, SymbolFilter,
};
use crate::var_decl::var_decl_info_for_symbol;

struct SymbolRenderContext<'a> {
    source: &'a str,
    root: &'a SyntaxNode,
    range: TextRange,
}

/// Result of a hover request.
#[derive(Debug, Clone)]
pub struct HoverResult {
    /// The hover content (markdown).
    pub contents: String,
    /// The range of the hovered element.
    pub range: Option<TextRange>,
}

impl HoverResult {
    /// Creates a new hover result.
    pub fn new(contents: impl Into<String>) -> Self {
        Self {
            contents: contents.into(),
            range: None,
        }
    }

    /// Sets the range.
    #[must_use]
    pub fn with_range(mut self, range: TextRange) -> Self {
        self.range = Some(range);
        self
    }
}

/// Computes hover information at the given position.
pub fn hover(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
) -> Option<HoverResult> {
    hover_with_filter(db, file_id, position, &StdlibFilter::allow_all())
}

/// Computes hover information with stdlib filtering.
pub fn hover_with_filter(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
    stdlib_filter: &StdlibFilter,
) -> Option<HoverResult> {
    let context = IdeContext::new(db, file_id);
    if let Some(result) = hover_task_priority(&context, position) {
        return Some(result);
    }
    if let Some(result) = hover_typed_literal(&context, position) {
        return Some(result);
    }
    if has_ambiguous_reference(db, file_id, position) {
        if let Some(result) = hover_ambiguous_using(&context, position) {
            return Some(result);
        }
    }
    let target = context.resolve_target_at_position(position);
    match target {
        Some(ResolvedTarget::Symbol(symbol_id)) => {
            let symbols = &context.symbols;
            let symbol = symbols.get(symbol_id)?;
            let (symbol_source, symbol_root, symbol_range) = if let Some(origin) = symbol.origin {
                let origin_source = db.source_text(origin.file_id);
                let origin_parsed = parse(&origin_source);
                let origin_symbols = db.file_symbols(origin.file_id);
                let origin_range = origin_symbols
                    .get(origin.symbol_id)
                    .map(|sym| sym.range)
                    .unwrap_or(symbol.range);
                (origin_source, origin_parsed.syntax(), origin_range)
            } else {
                (context.source.clone(), context.root.clone(), symbol.range)
            };
            let type_name = type_name_for_id(symbols, symbol.type_id);
            let scope_id = scope_at_position(symbols, &context.root, position);
            let render = SymbolRenderContext {
                source: &symbol_source,
                root: &symbol_root,
                range: symbol_range,
            };
            let contents = format_symbol(
                symbol,
                symbols,
                &render,
                type_name.as_deref(),
                scope_id,
                stdlib_filter,
            );
            let range = context
                .root
                .token_at_offset(position)
                .right_biased()?
                .text_range();
            Some(HoverResult::new(contents).with_range(range))
        }
        Some(ResolvedTarget::Field(field)) => {
            let symbols = &context.symbols;
            let field_type = field_type(symbols, &field)?;
            let field_type_name =
                type_name_for_id(symbols, field_type).unwrap_or_else(|| "?".to_string());
            let contents = format_field(&field.name, &field_type_name);
            let range = context
                .root
                .token_at_offset(position)
                .right_biased()?
                .text_range();
            Some(HoverResult::new(contents).with_range(range))
        }
        None => hover_ambiguous_using(&context, position)
            .or_else(|| hover_standard_function(&context, position, stdlib_filter)),
    }
}

fn hover_task_priority(context: &IdeContext<'_>, position: TextSize) -> Option<HoverResult> {
    let token = context.root.token_at_offset(position).right_biased()?;
    if !token.text().eq_ignore_ascii_case("PRIORITY") {
        return None;
    }
    if !token_has_task_init_parent(&token) {
        return None;
    }

    let contents = concat!(
        "```st\nPRIORITY : UINT\n```\n",
        "0 = highest priority; larger numbers = lower priority.\n",
        "Scheduling policy is runtime-defined (preemptive or non-preemptive).\n",
        "For non-preemptive scheduling, the longest-waiting task at the highest priority runs first."
    );
    Some(HoverResult::new(contents).with_range(token.text_range()))
}

fn token_has_task_init_parent(token: &SyntaxToken) -> bool {
    let Some(parent) = token.parent() else {
        return false;
    };
    parent
        .ancestors()
        .any(|node| node.kind() == SyntaxKind::TaskInit)
}

fn hover_ambiguous_using(context: &IdeContext<'_>, position: TextSize) -> Option<HoverResult> {
    let (name, range) = ident_at_offset(&context.source, position)?;
    let scope_id = scope_at_position(&context.symbols, &context.root, position);
    let candidates = collect_using_candidates(&context.symbols, scope_id, name);
    if candidates.len() <= 1 {
        return None;
    }

    let mut candidate_names = candidates
        .iter()
        .map(|parts| join_namespace_path(parts))
        .collect::<Vec<_>>();
    candidate_names.sort();
    candidate_names.dedup();

    let mut using_paths = candidates
        .iter()
        .filter_map(|parts| {
            if parts.len() > 1 {
                Some(join_namespace_path(&parts[..parts.len() - 1]))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    using_paths.sort();
    using_paths.dedup();

    let mut contents = format!("```st\n{name}\n```\n\nAmbiguous reference to `{name}`.");
    let mut sections = Vec::new();
    sections.push(format!("Candidates:\n- {}", candidate_names.join("\n- ")));
    if !using_paths.is_empty() {
        sections.push(format!("USING:\n- {}", using_paths.join("\n- ")));
    }
    if !sections.is_empty() {
        contents.push_str("\n\n---\n\n");
        contents.push_str(&sections.join("\n\n"));
    }

    Some(HoverResult::new(contents).with_range(range))
}

fn has_ambiguous_reference(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
) -> bool {
    let diagnostics = db.diagnostics(file_id);
    diagnostics.iter().any(|diag| {
        diag.code == DiagnosticCode::CannotResolve
            && diag.message.contains("ambiguous reference to")
            && diag.range.contains(position)
    })
}

fn collect_using_candidates(
    symbols: &SymbolTable,
    scope_id: ScopeId,
    name: &str,
) -> Vec<Vec<SmolStr>> {
    let mut candidates = Vec::new();
    let mut current = Some(scope_id);
    while let Some(scope_id) = current {
        let Some(scope) = symbols.get_scope(scope_id) else {
            break;
        };
        if scope.lookup_local(name).is_some() {
            break;
        }
        for using in &scope.using_directives {
            let mut parts = using.path.clone();
            parts.push(SmolStr::new(name));
            let Some(symbol_id) = symbols.resolve_qualified(&parts) else {
                continue;
            };
            if let Some(symbol) = symbols.get(symbol_id) {
                if matches!(symbol.kind, SymbolKind::Namespace) {
                    continue;
                }
            }
            candidates.push(parts);
        }
        current = scope.parent;
    }

    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for parts in candidates {
        let key = parts
            .iter()
            .map(|part| part.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join(".");
        if seen.insert(key) {
            unique.push(parts);
        }
    }
    unique
}

fn join_namespace_path(parts: &[SmolStr]) -> String {
    let mut out = String::new();
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            out.push('.');
        }
        out.push_str(part.as_str());
    }
    out
}

/// Formats a symbol for hover display.
fn format_symbol(
    symbol: &Symbol,
    symbols: &SymbolTable,
    render: &SymbolRenderContext<'_>,
    type_name: Option<&str>,
    scope_id: trust_hir::symbols::ScopeId,
    stdlib_filter: &StdlibFilter,
) -> String {
    let mut result = String::new();

    // Add code block with signature
    result.push_str("```st\n");
    let visibility = visibility_prefix(symbol.visibility);
    let modifiers = modifiers_prefix(symbol.modifiers);
    let header_prefix = format_symbol_prefix(visibility, modifiers.as_deref());

    match &symbol.kind {
        SymbolKind::Variable { qualifier } => {
            let info = var_decl_info_for_symbol(render.root, render.source, render.range);
            let mut qual = match qualifier {
                VarQualifier::Input => "VAR_INPUT",
                VarQualifier::Output => "VAR_OUTPUT",
                VarQualifier::InOut => "VAR_IN_OUT",
                VarQualifier::Local => "VAR",
                VarQualifier::Temp => "VAR_TEMP",
                VarQualifier::Global => "VAR_GLOBAL",
                VarQualifier::External => "VAR_EXTERNAL",
                VarQualifier::Access => "VAR_ACCESS",
                VarQualifier::Static => "VAR_STAT",
            }
            .to_string();
            if let Some(retention) = info.retention {
                qual.push(' ');
                qual.push_str(retention);
            }
            result.push_str(&format!(
                "({}) {} : {}",
                qual,
                symbol.name,
                type_name.unwrap_or("?")
            ));
            if let Some(initializer) = info.initializer {
                result.push_str(&format!(" := {}", initializer));
            }
        }
        SymbolKind::Constant => {
            let info = var_decl_info_for_symbol(render.root, render.source, render.range);
            result.push_str(&format!(
                "(CONSTANT) {}{} : {}",
                header_prefix,
                symbol.name,
                type_name.unwrap_or("?")
            ));
            if let Some(initializer) = info.initializer {
                result.push_str(&format!(" := {}", initializer));
            }
        }
        SymbolKind::Function { .. } => {
            result.push_str(&format!(
                "FUNCTION {}{} : {}",
                header_prefix,
                symbol.name,
                type_name.unwrap_or("?")
            ));
        }
        SymbolKind::FunctionBlock => {
            result.push_str(&format_function_block(
                symbol,
                symbols,
                render.root,
                render.source,
                render.range,
                &header_prefix,
            ));
        }
        SymbolKind::Class => {
            let mut header = format!("CLASS {}{}", header_prefix, symbol.name);
            for line in inheritance_lines(symbols, render.root, symbol, render.range) {
                header.push('\n');
                header.push_str(&line);
            }
            result.push_str(&header);
        }
        SymbolKind::Method { return_type, .. } => {
            if return_type.is_some() {
                result.push_str(&format!(
                    "METHOD {}{} : {}",
                    header_prefix,
                    symbol.name,
                    type_name.unwrap_or("?")
                ));
            } else {
                result.push_str(&format!("METHOD {}{}", header_prefix, symbol.name));
            }
        }
        SymbolKind::Property {
            has_get, has_set, ..
        } => {
            let access = match (has_get, has_set) {
                (true, true) => "GET/SET",
                (true, false) => "GET",
                (false, true) => "SET",
                (false, false) => "",
            };
            result.push_str(&format!(
                "PROPERTY {}{} : {} [{}]",
                header_prefix,
                symbol.name,
                type_name.unwrap_or("?"),
                access
            ));
        }
        SymbolKind::Interface => {
            let mut header = format!("INTERFACE {}{}", header_prefix, symbol.name);
            for line in inheritance_lines(symbols, render.root, symbol, render.range) {
                header.push('\n');
                header.push_str(&line);
            }
            result.push_str(&header);
        }
        SymbolKind::Namespace => {
            result.push_str(&format!("NAMESPACE {}{}", header_prefix, symbol.name));
        }
        SymbolKind::Program => {
            result.push_str(&format!("PROGRAM {}{}", header_prefix, symbol.name));
        }
        SymbolKind::Configuration => {
            result.push_str(&format!("CONFIGURATION {}{}", header_prefix, symbol.name));
        }
        SymbolKind::Resource => {
            let resource_type = resource_type_for_symbol(render.root, render.source, render.range);
            if let Some(resource_type) = resource_type {
                result.push_str(&format!(
                    "RESOURCE {}{} ON {}",
                    header_prefix, symbol.name, resource_type
                ));
            } else {
                result.push_str(&format!("RESOURCE {}{}", header_prefix, symbol.name));
            }
        }
        SymbolKind::Task => {
            let task_init =
                task_init_for_symbol(render.root, render.source, render.range).unwrap_or_default();
            if task_init.is_empty() {
                result.push_str(&format!("TASK {}{}", header_prefix, symbol.name));
            } else {
                result.push_str(&format!(
                    "TASK {}{} ({})",
                    header_prefix, symbol.name, task_init
                ));
            }
        }
        SymbolKind::ProgramInstance => {
            let (type_name, task_name, retain) =
                program_config_details(render.root, render.source, render.range);
            let mut header = format!(
                "PROGRAM {}{} : {}",
                header_prefix,
                symbol.name,
                type_name.unwrap_or_else(|| "?".to_string())
            );
            if let Some(task) = task_name {
                header.push_str(&format!(" WITH {task}"));
            }
            if let Some(retain) = retain {
                header.push_str(&format!(" [{retain}]"));
            }
            result.push_str(&header);
        }
        SymbolKind::Type => {
            result.push_str(&format_type_definition(symbols, symbol));
        }
        SymbolKind::EnumValue { value } => {
            result.push_str(&format!("{}{} := {}", header_prefix, symbol.name, value));
        }
        SymbolKind::Parameter { direction } => {
            let dir = match direction {
                trust_hir::symbols::ParamDirection::In => "IN",
                trust_hir::symbols::ParamDirection::Out => "OUT",
                trust_hir::symbols::ParamDirection::InOut => "IN_OUT",
            };
            result.push_str(&format!(
                "({}) {}{} : {}",
                dir,
                header_prefix,
                symbol.name,
                type_name.unwrap_or("?")
            ));
        }
    }

    result.push_str("\n```");

    let mut sections = Vec::new();
    if let Some(doc) = &symbol.doc {
        sections.push(doc.to_string());
    } else if stdlib_filter.allows_function_block(symbol.name.as_str()) {
        if let Some(std_doc) = stdlib_docs::standard_fb_doc(symbol.name.as_str()) {
            sections.push(std_doc.to_string());
        }
    }

    let ns_parts = namespace_path_for_symbol(symbols, symbol);
    if !ns_parts.is_empty() {
        let namespace = ns_parts
            .iter()
            .map(|part| part.as_str())
            .collect::<Vec<_>>()
            .join(".");
        sections.push(format!("Namespace: {namespace}"));
    }
    if let Some(using_path) =
        using_path_for_symbol(symbols, scope_id, symbol.name.as_str(), symbol.id)
    {
        let using = using_path
            .iter()
            .map(|part| part.as_str())
            .collect::<Vec<_>>()
            .join(".");
        sections.push(format!("USING {using}"));
    }

    if !sections.is_empty() {
        result.push_str("\n\n---\n\n");
        result.push_str(&sections.join("\n\n"));
    }

    result
}

fn format_symbol_prefix(visibility: Option<&str>, modifiers: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(vis) = visibility {
        parts.push(vis);
    }
    if let Some(mods) = modifiers {
        parts.push(mods);
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{} ", parts.join(" "))
    }
}

fn visibility_prefix(visibility: Visibility) -> Option<&'static str> {
    match visibility {
        Visibility::Public => None,
        Visibility::Private => Some("PRIVATE"),
        Visibility::Protected => Some("PROTECTED"),
        Visibility::Internal => Some("INTERNAL"),
    }
}

fn modifiers_prefix(modifiers: SymbolModifiers) -> Option<String> {
    let mut parts = Vec::new();
    if modifiers.is_final {
        parts.push("FINAL");
    }
    if modifiers.is_abstract {
        parts.push("ABSTRACT");
    }
    if modifiers.is_override {
        parts.push("OVERRIDE");
    }
    (!parts.is_empty()).then_some(parts.join(" "))
}

fn hover_typed_literal(context: &IdeContext<'_>, position: TextSize) -> Option<HoverResult> {
    let offset = u32::from(position) as usize;
    let mut pending_prefix: Option<(String, TextRange, bool)> = None;

    for token in lex(&context.source) {
        let start = usize::from(token.range.start());
        let end = usize::from(token.range.end());

        if matches!(
            token.kind,
            TokenKind::TimeLiteral
                | TokenKind::DateLiteral
                | TokenKind::TimeOfDayLiteral
                | TokenKind::DateAndTimeLiteral
        ) && start <= offset
            && offset < end
        {
            let text = &context.source[start..end];
            let (prefix, _) = text.split_once('#')?;
            let doc = stdlib_docs::typed_literal_doc(prefix)?;
            let contents = format!("```st\n{}\n```\n\n{}", text, doc);
            return Some(HoverResult::new(contents).with_range(token.range));
        }

        match token.kind {
            TokenKind::TypedLiteralPrefix => {
                let text = context.source[start..end].to_string();
                let cursor_in_prefix = start <= offset && offset < end;
                pending_prefix = Some((text, token.range, cursor_in_prefix));
            }
            TokenKind::IntLiteral
            | TokenKind::RealLiteral
            | TokenKind::StringLiteral
            | TokenKind::WideStringLiteral
            | TokenKind::TimeLiteral
            | TokenKind::DateLiteral
            | TokenKind::TimeOfDayLiteral
            | TokenKind::DateAndTimeLiteral
            | TokenKind::KwTrue
            | TokenKind::KwFalse
            | TokenKind::Ident => {
                if let Some((prefix_text, prefix_range, cursor_in_prefix)) = pending_prefix.take() {
                    let cursor_in_value = start <= offset && offset < end;
                    if cursor_in_prefix || cursor_in_value {
                        let value_text = &context.source[start..end];
                        let literal_text = format!("{}{}", prefix_text, value_text);
                        let prefix = prefix_text.trim_end_matches('#');
                        let doc = stdlib_docs::typed_literal_doc(prefix)?;
                        let range = TextRange::new(prefix_range.start(), token.range.end());
                        let contents = format!("```st\n{}\n```\n\n{}", literal_text, doc);
                        return Some(HoverResult::new(contents).with_range(range));
                    }
                }
            }
            _ => {
                if !token.kind.is_trivia() {
                    pending_prefix = None;
                }
            }
        }
    }

    None
}

fn hover_standard_function(
    context: &IdeContext<'_>,
    position: TextSize,
    stdlib_filter: &StdlibFilter,
) -> Option<HoverResult> {
    let (name, range) = ident_at_offset(&context.source, position)?;
    if !stdlib_filter.allows_function(name) {
        return None;
    }
    let doc = stdlib_docs::standard_function_doc(name)?;
    let signature = signature_help(context.db, context.file_id, position)
        .and_then(|help| help.signatures.first().map(|sig| sig.label.clone()))
        .unwrap_or_else(|| name.to_string());
    let contents = format!("```st\n{signature}\n```\n\n{doc}");
    Some(HoverResult::new(contents).with_range(range))
}

/// Formats a type for hover display.
pub fn format_type(ty: &Type) -> String {
    match ty {
        Type::Bool => "BOOL".to_string(),
        Type::SInt => "SINT".to_string(),
        Type::Int => "INT".to_string(),
        Type::DInt => "DINT".to_string(),
        Type::LInt => "LINT".to_string(),
        Type::USInt => "USINT".to_string(),
        Type::UInt => "UINT".to_string(),
        Type::UDInt => "UDINT".to_string(),
        Type::ULInt => "ULINT".to_string(),
        Type::Real => "REAL".to_string(),
        Type::LReal => "LREAL".to_string(),
        Type::Byte => "BYTE".to_string(),
        Type::Word => "WORD".to_string(),
        Type::DWord => "DWORD".to_string(),
        Type::LWord => "LWORD".to_string(),
        Type::Time => "TIME".to_string(),
        Type::LTime => "LTIME".to_string(),
        Type::Date => "DATE".to_string(),
        Type::LDate => "LDATE".to_string(),
        Type::Tod => "TIME_OF_DAY".to_string(),
        Type::LTod => "LTIME_OF_DAY".to_string(),
        Type::Dt => "DATE_AND_TIME".to_string(),
        Type::Ldt => "LDATE_AND_TIME".to_string(),
        Type::Char => "CHAR".to_string(),
        Type::WChar => "WCHAR".to_string(),
        Type::String { max_len } => {
            if let Some(len) = max_len {
                format!("STRING[{}]", len)
            } else {
                "STRING".to_string()
            }
        }
        Type::WString { max_len } => {
            if let Some(len) = max_len {
                format!("WSTRING[{}]", len)
            } else {
                "WSTRING".to_string()
            }
        }
        Type::Array { dimensions, .. } => {
            let dims: Vec<String> = dimensions
                .iter()
                .map(|(l, u)| format!("{}..{}", l, u))
                .collect();
            format!("ARRAY[{}] OF ...", dims.join(", "))
        }
        Type::Struct { name, .. } => format!("STRUCT {}", name),
        Type::Union { name, .. } => format!("UNION {}", name),
        Type::Enum { name, .. } => name.to_string(),
        Type::Pointer { .. } => "POINTER TO ...".to_string(),
        Type::Reference { .. } => "REF_TO ...".to_string(),
        Type::Subrange { base, lower, upper } => {
            let base_name = TypeId::builtin_name(*base).unwrap_or("?");
            format!("{}({}..{})", base_name, lower, upper)
        }
        Type::FunctionBlock { name } => name.to_string(),
        Type::Class { name } => name.to_string(),
        Type::Interface { name } => name.to_string(),
        Type::Alias { name, .. } => name.to_string(),
        Type::Any => "ANY".to_string(),
        Type::AnyDerived => "ANY_DERIVED".to_string(),
        Type::AnyElementary => "ANY_ELEMENTARY".to_string(),
        Type::AnyMagnitude => "ANY_MAGNITUDE".to_string(),
        Type::AnyInt => "ANY_INT".to_string(),
        Type::AnyUnsigned => "ANY_UNSIGNED".to_string(),
        Type::AnySigned => "ANY_SIGNED".to_string(),
        Type::AnyReal => "ANY_REAL".to_string(),
        Type::AnyNum => "ANY_NUM".to_string(),
        Type::AnyDuration => "ANY_DURATION".to_string(),
        Type::AnyBit => "ANY_BIT".to_string(),
        Type::AnyChars => "ANY_CHARS".to_string(),
        Type::AnyString => "ANY_STRING".to_string(),
        Type::AnyChar => "ANY_CHAR".to_string(),
        Type::AnyDate => "ANY_DATE".to_string(),
        Type::Unknown => "?".to_string(),
        Type::Void => "VOID".to_string(),
        Type::Null => "NULL".to_string(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum VarSectionKind {
    Input,
    Output,
    InOut,
    Var,
    VarTemp,
    VarStat,
    VarGlobal,
    VarExternal,
    VarAccess,
    Constant,
}

impl VarSectionKind {
    fn header(self) -> &'static str {
        match self {
            VarSectionKind::Input => "VAR_INPUT",
            VarSectionKind::Output => "VAR_OUTPUT",
            VarSectionKind::InOut => "VAR_IN_OUT",
            VarSectionKind::Var => "VAR",
            VarSectionKind::VarTemp => "VAR_TEMP",
            VarSectionKind::VarStat => "VAR_STAT",
            VarSectionKind::VarGlobal => "VAR_GLOBAL",
            VarSectionKind::VarExternal => "VAR_EXTERNAL",
            VarSectionKind::VarAccess => "VAR_ACCESS",
            VarSectionKind::Constant => "VAR CONSTANT",
        }
    }
}

fn format_function_block(
    symbol: &Symbol,
    symbols: &SymbolTable,
    root: &SyntaxNode,
    source: &str,
    symbol_range: TextRange,
    header_prefix: &str,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("FUNCTION_BLOCK {}{}", header_prefix, symbol.name));
    for line in inheritance_lines(symbols, root, symbol, symbol_range) {
        lines.push(line);
    }

    let mut sections: std::collections::HashMap<VarSectionKind, Vec<(u32, String)>> =
        std::collections::HashMap::new();

    let filter = SymbolFilter::new(symbols);
    for member in filter.members_of_owner(symbol.id) {
        let section = match member.kind {
            SymbolKind::Parameter { direction } => match direction {
                trust_hir::symbols::ParamDirection::In => VarSectionKind::Input,
                trust_hir::symbols::ParamDirection::Out => VarSectionKind::Output,
                trust_hir::symbols::ParamDirection::InOut => VarSectionKind::InOut,
            },
            SymbolKind::Variable { qualifier } => match qualifier {
                VarQualifier::Local => VarSectionKind::Var,
                VarQualifier::Temp => VarSectionKind::VarTemp,
                VarQualifier::Static => VarSectionKind::VarStat,
                VarQualifier::Global => VarSectionKind::VarGlobal,
                VarQualifier::External => VarSectionKind::VarExternal,
                VarQualifier::Access => VarSectionKind::VarAccess,
                VarQualifier::Input => VarSectionKind::Input,
                VarQualifier::Output => VarSectionKind::Output,
                VarQualifier::InOut => VarSectionKind::InOut,
            },
            SymbolKind::Constant => VarSectionKind::Constant,
            _ => continue,
        };

        let type_name =
            type_name_for_id(symbols, member.type_id).unwrap_or_else(|| "?".to_string());
        let mut line = format!("    {} : {}", member.name, type_name);
        if let Some(initializer) = var_decl_info_for_symbol(root, source, member.range).initializer
        {
            line.push_str(&format!(" := {}", initializer));
        }
        line.push(';');
        sections
            .entry(section)
            .or_default()
            .push((u32::from(member.range.start()), line));
    }

    let section_order = [
        VarSectionKind::Input,
        VarSectionKind::Output,
        VarSectionKind::InOut,
        VarSectionKind::Var,
        VarSectionKind::VarTemp,
        VarSectionKind::VarStat,
        VarSectionKind::VarGlobal,
        VarSectionKind::VarExternal,
        VarSectionKind::VarAccess,
        VarSectionKind::Constant,
    ];

    for section in section_order {
        let Some(mut entries) = sections.remove(&section) else {
            continue;
        };
        entries.sort_by_key(|(start, _)| *start);
        lines.push(section.header().to_string());
        for (_, line) in entries {
            lines.push(line);
        }
        lines.push("END_VAR".to_string());
    }

    lines.push("END_FUNCTION_BLOCK".to_string());
    lines.join("\n")
}

fn inheritance_lines(
    symbols: &SymbolTable,
    root: &SyntaxNode,
    symbol: &Symbol,
    symbol_range: TextRange,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(base) = symbols.extends_name(symbol.id) {
        lines.push(format!("EXTENDS {}", base));
    }
    if matches!(symbol.kind, SymbolKind::FunctionBlock | SymbolKind::Class) {
        let implements = implements_names_for_symbol(root, symbol, symbol_range);
        if !implements.is_empty() {
            lines.push(format!("IMPLEMENTS {}", implements.join(", ")));
        }
    }
    lines
}

fn implements_names_for_symbol(
    root: &SyntaxNode,
    symbol: &Symbol,
    symbol_range: TextRange,
) -> Vec<String> {
    let kind = match symbol.kind {
        SymbolKind::FunctionBlock => SyntaxKind::FunctionBlock,
        SymbolKind::Class => SyntaxKind::Class,
        _ => return Vec::new(),
    };
    let Some(node) = find_named_node(root, symbol_range, kind) else {
        return Vec::new();
    };
    let Some(clause) = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ImplementsClause)
    else {
        return Vec::new();
    };
    qualified_names_in_clause(&clause)
}

fn resource_type_for_symbol(
    root: &SyntaxNode,
    _source: &str,
    symbol_range: TextRange,
) -> Option<String> {
    let node = find_named_node(root, symbol_range, SyntaxKind::Resource)?;
    let mut saw_on = false;
    for element in node.children_with_tokens() {
        if let Some(token) = element.as_token() {
            if token.kind() == SyntaxKind::KwOn {
                saw_on = true;
                continue;
            }
        }
        if saw_on {
            if let Some(child) = element
                .as_node()
                .filter(|node| matches!(node.kind(), SyntaxKind::QualifiedName | SyntaxKind::Name))
            {
                if let Some(name) = qualified_name_text(child) {
                    return Some(name);
                }
            }
        }
    }
    None
}

fn task_init_for_symbol(
    root: &SyntaxNode,
    source: &str,
    symbol_range: TextRange,
) -> Option<String> {
    let node = find_named_node(root, symbol_range, SyntaxKind::TaskConfig)?;
    let init = node
        .children()
        .find(|child| child.kind() == SyntaxKind::TaskInit)?;
    let mut parts = Vec::new();
    let elements: Vec<SyntaxElement> = init.children_with_tokens().collect();
    let mut idx = 0;
    while idx < elements.len() {
        let Some(name_node) = elements[idx]
            .as_node()
            .filter(|node| node.kind() == SyntaxKind::Name)
        else {
            idx += 1;
            continue;
        };
        let Some(assign) = elements
            .get(idx + 1)
            .and_then(|element| element.as_token())
            .filter(|token| token.kind() == SyntaxKind::Assign)
        else {
            idx += 1;
            continue;
        };
        let _ = assign;
        let Some(name) = qualified_name_text(name_node) else {
            idx += 1;
            continue;
        };
        let mut expr_range = None;
        let mut j = idx + 2;
        while j < elements.len() {
            if let Some(node) = elements[j].as_node() {
                expr_range = Some(node.text_range());
                break;
            }
            if let Some(token) = elements[j].as_token() {
                if matches!(token.kind(), SyntaxKind::Comma | SyntaxKind::RParen) {
                    break;
                }
            }
            j += 1;
        }
        if let Some(range) = expr_range {
            if let Some(expr_text) = slice_source(source, range) {
                parts.push(format!("{name} := {expr_text}"));
            }
        }
        idx = j;
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn program_config_details(
    root: &SyntaxNode,
    source: &str,
    symbol_range: TextRange,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(node) = find_named_node(root, symbol_range, SyntaxKind::ProgramConfig) else {
        return (None, None, None);
    };

    let mut retain = None;
    let mut task = None;
    let mut type_name = None;
    let mut saw_colon = false;
    let mut saw_with = false;

    for element in node.children_with_tokens() {
        if let Some(token) = element.as_token() {
            match token.kind() {
                SyntaxKind::KwRetain => retain = Some("RETAIN".to_string()),
                SyntaxKind::KwNonRetain => retain = Some("NON_RETAIN".to_string()),
                SyntaxKind::KwWith => {
                    saw_with = true;
                }
                SyntaxKind::Colon => {
                    saw_colon = true;
                }
                _ => {}
            }
        }
        if let Some(child) = element.as_node() {
            if saw_with && task.is_none() && child.kind() == SyntaxKind::Name {
                task = qualified_name_text(child);
                saw_with = false;
            } else if saw_colon
                && type_name.is_none()
                && matches!(
                    child.kind(),
                    SyntaxKind::QualifiedName | SyntaxKind::TypeRef | SyntaxKind::Name
                )
            {
                type_name = qualified_name_text(child)
                    .or_else(|| slice_source(source, child.text_range()).map(|s| s.to_string()));
                saw_colon = false;
            }
        }
    }

    (type_name, task, retain)
}

fn qualified_name_text(node: &SyntaxNode) -> Option<String> {
    let target = match node.kind() {
        SyntaxKind::QualifiedName | SyntaxKind::Name => node.clone(),
        SyntaxKind::TypeRef => node
            .children()
            .find(|child| matches!(child.kind(), SyntaxKind::QualifiedName | SyntaxKind::Name))?,
        _ => return None,
    };
    let mut parts = Vec::new();
    for child in target
        .children()
        .filter(|child| child.kind() == SyntaxKind::Name)
    {
        if let Some(ident) = ident_token_in_name(&child) {
            parts.push(ident.text().to_string());
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fn slice_source(source: &str, range: TextRange) -> Option<&str> {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    source.get(start..end)
}

#[cfg(test)]
mod hover_docs_tests {
    use super::*;
    use trust_hir::db::{Database, FileId, SourceDatabase};

    #[test]
    fn test_hover_standard_function_doc() {
        let source = r#"
PROGRAM Main
VAR
    x : INT;
END_VAR
    x := AB|S(1);
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let result = hover(&db, file_id, TextSize::from(cursor as u32)).expect("hover result");
        assert!(result.contents.contains("IEC 61131-3"));
    }

    #[test]
    fn test_hover_typed_literal_doc() {
        let source = r#"
PROGRAM Main
VAR
    x : TIME;
END_VAR
    x := T#|1s;
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let result = hover(&db, file_id, TextSize::from(cursor as u32)).expect("hover result");
        assert!(result.contents.contains("Table 8"));
    }

    #[test]
    fn test_hover_namespace_using_info() {
        let source = r#"
NAMESPACE Lib
FUNCTION Foo : INT
END_FUNCTION
END_NAMESPACE

PROGRAM Main
USING Lib;
VAR
    x : INT;
END_VAR
    x := Fo|o();
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let result = hover(&db, file_id, TextSize::from(cursor as u32)).expect("hover result");
        assert!(result.contents.contains("Namespace: Lib"));
        assert!(result.contents.contains("USING Lib"));
    }

    #[test]
    fn test_hover_namespace_ambiguity_info() {
        let source = r#"
NAMESPACE LibA
FUNCTION Foo : INT
END_FUNCTION
END_NAMESPACE

NAMESPACE LibB
FUNCTION Foo : INT
END_FUNCTION
END_NAMESPACE

PROGRAM Main
USING LibA;
USING LibB;
VAR
    x : INT;
END_VAR
    x := Fo|o();
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let result = hover(&db, file_id, TextSize::from(cursor as u32)).expect("hover result");
        assert!(result.contents.contains("Ambiguous reference"));
        assert!(result.contents.contains("LibA.Foo"));
        assert!(result.contents.contains("LibB.Foo"));
        assert!(result.contents.contains("USING"));
    }
}

fn qualified_names_in_clause(clause: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    for node in clause
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::QualifiedName)
    {
        if let Some(name) = qualified_name_from_node(&node) {
            names.push(name);
        }
    }
    if !names.is_empty() {
        return names;
    }

    for node in clause
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::Name)
    {
        if node
            .parent()
            .is_some_and(|parent| parent.kind() == SyntaxKind::QualifiedName)
        {
            continue;
        }
        if let Some(name) = qualified_name_from_node(&node) {
            names.push(name);
        }
    }

    names
}

fn qualified_name_from_node(node: &SyntaxNode) -> Option<String> {
    let target = match node.kind() {
        SyntaxKind::QualifiedName => node.clone(),
        SyntaxKind::Name => {
            if let Some(parent) = node.parent() {
                if parent.kind() == SyntaxKind::QualifiedName {
                    parent
                } else {
                    node.clone()
                }
            } else {
                node.clone()
            }
        }
        _ => return None,
    };

    match target.kind() {
        SyntaxKind::Name => ident_token_in_name(&target).map(|token| token.text().to_string()),
        SyntaxKind::QualifiedName => {
            let mut parts = Vec::new();
            for child in target.children().filter(|n| n.kind() == SyntaxKind::Name) {
                if let Some(ident) = ident_token_in_name(&child) {
                    parts.push(ident.text().to_string());
                }
            }
            (!parts.is_empty()).then_some(parts.join("."))
        }
        _ => None,
    }
}

fn find_named_node(
    root: &SyntaxNode,
    symbol_range: TextRange,
    kind: SyntaxKind,
) -> Option<SyntaxNode> {
    root.descendants()
        .filter(|node| node.kind() == kind)
        .find(|node| name_range_from_node(node) == Some(symbol_range))
}

fn format_type_definition(symbols: &SymbolTable, symbol: &Symbol) -> String {
    let name = symbol.name.as_str();
    let Some(ty) = symbols.type_by_id(symbol.type_id) else {
        return format!("TYPE {}", name);
    };

    match ty {
        Type::Struct { fields, .. } => {
            let mut lines = Vec::new();
            lines.push(format!("TYPE {} :", name));
            lines.push("STRUCT".to_string());
            for field in fields.iter() {
                let field_type_name = format_type_ref(symbols, field.type_id);
                lines.push(format!("    {} : {};", field.name, field_type_name));
            }
            lines.push("END_STRUCT".to_string());
            lines.push("END_TYPE".to_string());
            lines.join("\n")
        }
        Type::Union { variants, .. } => {
            let mut lines = Vec::new();
            lines.push(format!("TYPE {} :", name));
            lines.push("UNION".to_string());
            for variant in variants.iter() {
                let field_type_name = format_type_ref(symbols, variant.type_id);
                lines.push(format!("    {} : {};", variant.name, field_type_name));
            }
            lines.push("END_UNION".to_string());
            lines.push("END_TYPE".to_string());
            lines.join("\n")
        }
        Type::Enum { values, .. } => {
            let mut lines = Vec::new();
            lines.push(format!("TYPE {} :", name));
            lines.push("(".to_string());
            for (idx, (value_name, value)) in values.iter().enumerate() {
                let mut line = format!("    {} := {}", value_name, value);
                if idx + 1 < values.len() {
                    line.push(',');
                }
                lines.push(line);
            }
            lines.push(");".to_string());
            lines.push("END_TYPE".to_string());
            lines.join("\n")
        }
        Type::Array {
            dimensions,
            element,
        } => {
            let dims: Vec<String> = dimensions
                .iter()
                .map(|(lower, upper)| format!("{}..{}", lower, upper))
                .collect();
            let element_name = format_type_ref(symbols, *element);
            format!(
                "TYPE {} : ARRAY[{}] OF {};\nEND_TYPE",
                name,
                dims.join(", "),
                element_name
            )
        }
        Type::Pointer { target } => format!(
            "TYPE {} : POINTER TO {};\nEND_TYPE",
            name,
            format_type_ref(symbols, *target)
        ),
        Type::Reference { target } => format!(
            "TYPE {} : REF_TO {};\nEND_TYPE",
            name,
            format_type_ref(symbols, *target)
        ),
        Type::Subrange { base, lower, upper } => {
            let base_name = format_type_ref(symbols, *base);
            format!(
                "TYPE {} : {}({}..{});\nEND_TYPE",
                name, base_name, lower, upper
            )
        }
        Type::Alias { target, .. } => format!(
            "TYPE {} : {};\nEND_TYPE",
            name,
            format_type_ref(symbols, *target)
        ),
        _ => format!(
            "TYPE {} : {};\nEND_TYPE",
            name,
            format_type_ref(symbols, symbol.type_id)
        ),
    }
}

fn format_type_ref(symbols: &SymbolTable, type_id: TypeId) -> String {
    if let Some(name) = TypeId::builtin_name(type_id) {
        return name.to_string();
    }
    match symbols.type_by_id(type_id) {
        Some(Type::Array {
            dimensions,
            element,
        }) => {
            let dims: Vec<String> = dimensions
                .iter()
                .map(|(lower, upper)| format!("{}..{}", lower, upper))
                .collect();
            format!(
                "ARRAY[{}] OF {}",
                dims.join(", "),
                format_type_ref(symbols, *element)
            )
        }
        Some(Type::Pointer { target }) => {
            format!("POINTER TO {}", format_type_ref(symbols, *target))
        }
        Some(Type::Reference { target }) => format!("REF_TO {}", format_type_ref(symbols, *target)),
        Some(Type::Subrange { base, lower, upper }) => {
            let base_name = format_type_ref(symbols, *base);
            format!("{}({}..{})", base_name, lower, upper)
        }
        Some(Type::Struct { name, .. })
        | Some(Type::Union { name, .. })
        | Some(Type::Enum { name, .. })
        | Some(Type::FunctionBlock { name })
        | Some(Type::Class { name })
        | Some(Type::Interface { name })
        | Some(Type::Alias { name, .. }) => name.to_string(),
        Some(other) => format_type(other),
        None => "?".to_string(),
    }
}

fn format_field(name: &str, type_name: &str) -> String {
    let mut result = String::new();
    result.push_str("```st\n");
    result.push_str(&format!("FIELD {} : {}", name, type_name));
    result.push_str("\n```");
    result
}

fn type_name_for_id(symbols: &SymbolTable, type_id: TypeId) -> Option<String> {
    if let Some(name) = TypeId::builtin_name(type_id) {
        return Some(name.to_string());
    }
    symbols
        .type_by_id(type_id)
        .map(|_| format_type_ref(symbols, type_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_type() {
        assert_eq!(format_type(&Type::Int), "INT");
        assert_eq!(format_type(&Type::Bool), "BOOL");
        assert_eq!(
            format_type(&Type::String { max_len: Some(80) }),
            "STRING[80]"
        );
    }
}
