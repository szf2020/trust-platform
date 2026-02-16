//! Code completion for Structured Text.
//!
//! This module provides context-aware completion suggestions.

use rustc_hash::FxHashSet;
use smol_str::SmolStr;
use text_size::{TextRange, TextSize};

use trust_hir::db::SemanticDatabase;
use trust_hir::symbols::{ParamDirection, ScopeId, SymbolId, SymbolTable, Visibility};
use trust_hir::{Database, SymbolKind, Type, TypeId};
use trust_syntax::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::signature_help::call_signature_context;
use crate::stdlib_docs::{self, StdlibFilter};
use crate::util::{
    is_member_symbol_kind, namespace_path_for_symbol, scope_at_position, type_detail,
    using_path_for_symbol, IdeContext, SymbolFilter,
};

/// The kind of completion item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// A keyword.
    Keyword,
    /// A function.
    Function,
    /// A function block.
    FunctionBlock,
    /// A method.
    Method,
    /// A property.
    Property,
    /// A variable.
    Variable,
    /// A constant.
    Constant,
    /// A type.
    Type,
    /// An enum value.
    EnumValue,
    /// A snippet.
    Snippet,
}

/// A completion item.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// The label shown in the completion list.
    pub label: SmolStr,
    /// The kind of completion.
    pub kind: CompletionKind,
    /// Additional detail (e.g., type signature).
    pub detail: Option<SmolStr>,
    /// Documentation.
    pub documentation: Option<SmolStr>,
    /// Text to insert (if different from label).
    pub insert_text: Option<SmolStr>,
    /// Text edit to apply (overrides insert_text when present).
    pub text_edit: Option<CompletionTextEdit>,
    /// Sort priority (lower = higher priority).
    pub sort_priority: u32,
}

impl CompletionItem {
    /// Creates a new completion item.
    pub fn new(label: impl Into<SmolStr>, kind: CompletionKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
            insert_text: None,
            text_edit: None,
            sort_priority: 100,
        }
    }

    /// Sets the detail text.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<SmolStr>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_documentation(mut self, doc: impl Into<SmolStr>) -> Self {
        self.documentation = Some(doc.into());
        self
    }

    /// Sets the insert text.
    #[must_use]
    pub fn with_insert_text(mut self, text: impl Into<SmolStr>) -> Self {
        self.insert_text = Some(text.into());
        self
    }

    /// Sets the text edit to apply.
    #[must_use]
    pub fn with_text_edit(mut self, edit: CompletionTextEdit) -> Self {
        self.text_edit = Some(edit);
        self
    }

    /// Sets the sort priority.
    #[must_use]
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.sort_priority = priority;
        self
    }
}

/// Text edit for completion items.
#[derive(Debug, Clone)]
pub struct CompletionTextEdit {
    /// The range to replace.
    pub range: TextRange,
    /// The new text to insert.
    pub new_text: SmolStr,
}

/// Context for completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionContext {
    /// At the start of a statement.
    Statement,
    /// After a dot (member access).
    MemberAccess,
    /// After a colon (type context).
    TypeAnnotation,
    /// Inside a call (parameter).
    Argument,
    /// Top level (outside any POU).
    TopLevel,
    /// Inside a VAR block.
    VarBlock,
    /// Unknown/general context.
    General,
}

/// Detects the completion context at a given position.
fn detect_context(root: &SyntaxNode, position: TextSize) -> CompletionContext {
    // Try to find the token at or just before the position
    let token = find_token_at_position(root, position);

    let Some(token) = token else {
        return CompletionContext::General;
    };

    // Check for trigger characters by looking at the previous non-trivia token
    if let Some(prev) = previous_non_trivia_token(&token) {
        match prev.kind() {
            // Dot triggers member access completion
            SyntaxKind::Dot => return CompletionContext::MemberAccess,
            // Colon triggers type annotation completion
            SyntaxKind::Colon => return CompletionContext::TypeAnnotation,
            // Comma in argument list
            SyntaxKind::Comma => {
                if is_in_argument_list(&prev) {
                    return CompletionContext::Argument;
                }
            }
            // Opening paren might be function call
            SyntaxKind::LParen => {
                if is_in_call_expr(&prev) {
                    return CompletionContext::Argument;
                }
            }
            _ => {}
        }
    }

    // Walk up ancestors to determine context
    for ancestor in token.parent_ancestors() {
        match ancestor.kind() {
            // Inside a type reference
            SyntaxKind::TypeRef => return CompletionContext::TypeAnnotation,

            // Inside extends/implements clause
            SyntaxKind::ExtendsClause | SyntaxKind::ImplementsClause => {
                return CompletionContext::TypeAnnotation;
            }

            // Inside a VAR block (but not in a type ref)
            SyntaxKind::VarBlock => {
                if is_recovered_statement_position(&ancestor, position) {
                    return CompletionContext::Statement;
                }
                return CompletionContext::VarBlock;
            }

            // Inside a VAR declaration (for type context)
            SyntaxKind::VarDecl => {
                if is_recovered_statement_position(&ancestor, position) {
                    return CompletionContext::Statement;
                }
                // Check if we're after the colon (type context)
                if has_colon_before_position(&ancestor, position) {
                    return CompletionContext::TypeAnnotation;
                }
                return CompletionContext::VarBlock;
            }

            // Inside an argument list
            SyntaxKind::ArgList => return CompletionContext::Argument,

            // Inside a statement list
            SyntaxKind::StmtList => return CompletionContext::Statement,

            // Inside a POU - we're in statement context
            SyntaxKind::Program
            | SyntaxKind::Function
            | SyntaxKind::FunctionBlock
            | SyntaxKind::Method => {
                // Only if we're past the VAR blocks
                if is_past_var_blocks(&ancestor, position) {
                    return CompletionContext::Statement;
                }
            }

            // At the source file level
            SyntaxKind::SourceFile => {
                // Check if we're inside a POU or at top level
                if !is_inside_pou(&ancestor, position) {
                    return CompletionContext::TopLevel;
                }
            }

            _ => {}
        }
    }

    CompletionContext::General
}

fn is_recovered_statement_position(node: &SyntaxNode, position: TextSize) -> bool {
    node.ancestors().any(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::Program
                | SyntaxKind::Function
                | SyntaxKind::FunctionBlock
                | SyntaxKind::Method
        ) && is_past_var_blocks(&ancestor, position)
    })
}

/// Finds the token at or just before a position.
fn find_token_at_position(root: &SyntaxNode, position: TextSize) -> Option<SyntaxToken> {
    // Try to get token at position, prefer right-biased
    if let Some(token) = root.token_at_offset(position).right_biased() {
        // If we're at the start of a token, return it
        if token.text_range().start() == position {
            return Some(token);
        }
        // If we're inside or at the end, return the previous token if position is at start
        return Some(token);
    }

    // If position is beyond file end, get the last token
    root.last_token()
}

/// Gets the previous non-trivia (non-whitespace, non-comment) token.
fn previous_non_trivia_token(token: &SyntaxToken) -> Option<SyntaxToken> {
    let mut prev = token.prev_token();
    while let Some(t) = prev {
        if !is_trivia(t.kind()) {
            return Some(t);
        }
        prev = t.prev_token();
    }
    None
}

/// Checks if a syntax kind is trivia (whitespace or comment).
fn is_trivia(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment
    )
}

/// Checks if the token is inside an argument list.
fn is_in_argument_list(token: &SyntaxToken) -> bool {
    token
        .parent_ancestors()
        .any(|n| n.kind() == SyntaxKind::ArgList)
}

/// Checks if the token is inside a call expression.
fn is_in_call_expr(token: &SyntaxToken) -> bool {
    token
        .parent_ancestors()
        .any(|n| n.kind() == SyntaxKind::CallExpr)
}

/// Checks if there's a colon before the position in the given node.
fn has_colon_before_position(node: &SyntaxNode, position: TextSize) -> bool {
    node.descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|t| t.kind() == SyntaxKind::Colon && t.text_range().end() <= position)
}

/// Checks if the position is past all VAR blocks in a POU.
fn is_past_var_blocks(pou: &SyntaxNode, position: TextSize) -> bool {
    // Find the last VAR block
    let last_var_block = pou
        .children()
        .filter(|n| n.kind() == SyntaxKind::VarBlock)
        .last();

    if let Some(var_block) = last_var_block {
        position > var_block.text_range().end()
    } else {
        // No VAR blocks, we're in statement context after the POU header
        // Check if we're past the POU name
        let pou_name_end = pou
            .children()
            .find(|n| n.kind() == SyntaxKind::Name)
            .map(|n| n.text_range().end())
            .unwrap_or(pou.text_range().start());
        position > pou_name_end
    }
}

/// Checks if the position is inside a POU.
fn is_inside_pou(source_file: &SyntaxNode, position: TextSize) -> bool {
    for child in source_file.children() {
        let is_pou = matches!(
            child.kind(),
            SyntaxKind::Program
                | SyntaxKind::Function
                | SyntaxKind::FunctionBlock
                | SyntaxKind::Method
                | SyntaxKind::Interface
        );
        if is_pou && child.text_range().contains(position) {
            return true;
        }
    }
    false
}

/// Computes completions at the given position.
pub fn complete(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
) -> Vec<CompletionItem> {
    complete_with_filter(db, file_id, position, &StdlibFilter::allow_all())
}

/// Computes completions with stdlib filtering.
pub fn complete_with_filter(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
    stdlib_filter: &StdlibFilter,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    let context = IdeContext::new(db, file_id);
    let root = &context.root;
    let symbols = &context.symbols;
    let filter = SymbolFilter::new(symbols);
    let detect = detect_context(root, position);
    let typed_literal_context = typed_literal_completion_context(&context, position);
    let scope_id = context.scope_at_position(position);

    match detect {
        CompletionContext::TopLevel => {
            items.extend(keyword_snippets());
        }
        CompletionContext::Statement => {
            items.extend(keyword_snippets());
            items.extend(symbols_in_scope(&filter, scope_id, stdlib_filter));
            items.extend(standard_function_completions(stdlib_filter));
            items.extend(typed_literal_completions_with_context(
                typed_literal_context.as_ref(),
            ));
        }
        CompletionContext::MemberAccess => {
            items.extend(member_access_completions(
                db,
                file_id,
                position,
                root,
                symbols,
                scope_id,
                stdlib_filter,
            ));
        }
        CompletionContext::TypeAnnotation => {
            items.extend(type_keywords());
            items.extend(type_symbols(&filter));
        }
        CompletionContext::VarBlock => {
            items.extend(keyword_snippets());
            items.extend(var_block_keywords());
        }
        CompletionContext::Argument => {
            items.extend(parameter_name_completions(db, file_id, position, symbols));
            items.extend(expression_keywords());
            items.extend(symbols_in_scope(&filter, scope_id, stdlib_filter));
            items.extend(standard_function_completions(stdlib_filter));
            items.extend(typed_literal_completions_with_context(
                typed_literal_context.as_ref(),
            ));
        }
        _ => {
            // General: include keywords and symbols
            items.extend(keyword_snippets());
            items.extend(expression_keywords());
            items.extend(symbols_in_scope(&filter, scope_id, stdlib_filter));
            items.extend(standard_function_completions(stdlib_filter));
            items.extend(typed_literal_completions_with_context(
                typed_literal_context.as_ref(),
            ));
        }
    }

    // Sort by priority
    items.sort_by_key(|item| item.sort_priority);
    items = dedupe_items(items);
    items
}

fn dedupe_items(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    let mut seen: FxHashSet<String> = FxHashSet::default();
    let mut deduped = Vec::new();
    for item in items {
        let key = item.label.to_ascii_uppercase();
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped
}

fn parameter_name_completions(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
    symbols: &SymbolTable,
) -> Vec<CompletionItem> {
    let Some(context) = call_signature_context(db, file_id, position) else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for param in context.signature.params {
        let key = SmolStr::new(param.name.to_ascii_uppercase());
        if context.used_params.contains(&key) {
            continue;
        }

        let op = match param.direction {
            ParamDirection::Out => "=>",
            ParamDirection::In | ParamDirection::InOut => ":=",
        };

        let type_name = type_detail(symbols, param.type_id)
            .map(|name| name.to_string())
            .or_else(|| param.type_id.builtin_name().map(|name| name.to_string()))
            .unwrap_or_else(|| "?".to_string());
        let direction = match param.direction {
            ParamDirection::In => "IN",
            ParamDirection::Out => "OUT",
            ParamDirection::InOut => "IN_OUT",
        };
        let detail = format!("{direction} : {type_name}");

        let mut item =
            CompletionItem::new(param.name.clone(), CompletionKind::Variable).with_priority(5);
        item.detail = Some(SmolStr::new(detail));
        item.insert_text = Some(SmolStr::new(format!("{} {} $0", param.name, op)));
        items.push(item);
    }

    items
}

fn keyword_snippets() -> Vec<CompletionItem> {
    let mut items = Vec::new();
    items.extend(top_level_keywords());
    items.extend(statement_keywords());
    items.extend(var_block_snippets());
    items.extend(vec![
        CompletionItem::new("NAMESPACE", CompletionKind::Keyword)
            .with_insert_text("NAMESPACE ${1:Name}\n\t$0\nEND_NAMESPACE")
            .with_priority(15),
        CompletionItem::new("STRUCT", CompletionKind::Keyword)
            .with_insert_text("STRUCT\n\t$0\nEND_STRUCT")
            .with_priority(15),
        CompletionItem::new("UNION", CompletionKind::Keyword)
            .with_insert_text("UNION\n\t$0\nEND_UNION")
            .with_priority(15),
        CompletionItem::new("METHOD", CompletionKind::Keyword)
            .with_insert_text("METHOD ${1:Name} : ${2:BOOL}\n\t$0\nEND_METHOD")
            .with_priority(15),
        CompletionItem::new("PROPERTY", CompletionKind::Keyword)
            .with_insert_text("PROPERTY ${1:Name} : ${2:INT}\nGET\n\t$0\nEND_GET\nEND_PROPERTY")
            .with_priority(15),
    ]);
    items
}

fn top_level_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("PROGRAM", CompletionKind::Keyword)
            .with_insert_text("PROGRAM ${1:Name}\n\t$0\nEND_PROGRAM")
            .with_priority(10),
        CompletionItem::new("FUNCTION", CompletionKind::Keyword)
            .with_insert_text("FUNCTION ${1:Name} : ${2:BOOL}\n\t$0\nEND_FUNCTION")
            .with_priority(10),
        CompletionItem::new("FUNCTION_BLOCK", CompletionKind::Keyword)
            .with_insert_text("FUNCTION_BLOCK ${1:Name}\n\t$0\nEND_FUNCTION_BLOCK")
            .with_priority(10),
        CompletionItem::new("CLASS", CompletionKind::Keyword)
            .with_insert_text("CLASS ${1:Name}\n\t$0\nEND_CLASS")
            .with_priority(10),
        CompletionItem::new("INTERFACE", CompletionKind::Keyword)
            .with_insert_text("INTERFACE ${1:I_Name}\n\t$0\nEND_INTERFACE")
            .with_priority(10),
        CompletionItem::new("CONFIGURATION", CompletionKind::Keyword)
            .with_insert_text("CONFIGURATION ${1:Name}\n\t$0\nEND_CONFIGURATION")
            .with_priority(10),
        CompletionItem::new("TYPE", CompletionKind::Keyword)
            .with_insert_text("TYPE ${1:Name} :\n\t$0\nEND_TYPE")
            .with_priority(10),
    ]
}

fn statement_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("IF", CompletionKind::Keyword)
            .with_insert_text("IF ${1:condition} THEN\n\t$0\nEND_IF")
            .with_priority(20),
        CompletionItem::new("CASE", CompletionKind::Keyword)
            .with_insert_text("CASE ${1:expression} OF\n\t${2:1}:\n\t\t$0\nEND_CASE")
            .with_priority(20),
        CompletionItem::new("FOR", CompletionKind::Keyword)
            .with_insert_text("FOR ${1:i} := ${2:0} TO ${3:10} DO\n\t$0\nEND_FOR")
            .with_priority(20),
        CompletionItem::new("WHILE", CompletionKind::Keyword)
            .with_insert_text("WHILE ${1:condition} DO\n\t$0\nEND_WHILE")
            .with_priority(20),
        CompletionItem::new("REPEAT", CompletionKind::Keyword)
            .with_insert_text("REPEAT\n\t$0\nUNTIL ${1:condition}\nEND_REPEAT")
            .with_priority(20),
        CompletionItem::new("RETURN", CompletionKind::Keyword).with_priority(25),
        CompletionItem::new("EXIT", CompletionKind::Keyword).with_priority(25),
        CompletionItem::new("CONTINUE", CompletionKind::Keyword).with_priority(25),
        CompletionItem::new("JMP", CompletionKind::Keyword).with_priority(25),
    ]
}

fn type_keywords() -> Vec<CompletionItem> {
    vec![
        // Boolean
        CompletionItem::new("BOOL", CompletionKind::Type).with_priority(30),
        // Integers
        CompletionItem::new("INT", CompletionKind::Type).with_priority(30),
        CompletionItem::new("DINT", CompletionKind::Type).with_priority(30),
        CompletionItem::new("SINT", CompletionKind::Type).with_priority(35),
        CompletionItem::new("LINT", CompletionKind::Type).with_priority(35),
        CompletionItem::new("UINT", CompletionKind::Type).with_priority(35),
        CompletionItem::new("UDINT", CompletionKind::Type).with_priority(35),
        CompletionItem::new("USINT", CompletionKind::Type).with_priority(40),
        CompletionItem::new("ULINT", CompletionKind::Type).with_priority(40),
        // Floating point
        CompletionItem::new("REAL", CompletionKind::Type).with_priority(30),
        CompletionItem::new("LREAL", CompletionKind::Type).with_priority(35),
        // Bit strings
        CompletionItem::new("BYTE", CompletionKind::Type).with_priority(35),
        CompletionItem::new("WORD", CompletionKind::Type).with_priority(35),
        CompletionItem::new("DWORD", CompletionKind::Type).with_priority(35),
        CompletionItem::new("LWORD", CompletionKind::Type).with_priority(40),
        // Strings
        CompletionItem::new("STRING", CompletionKind::Type).with_priority(30),
        CompletionItem::new("WSTRING", CompletionKind::Type).with_priority(35),
        CompletionItem::new("CHAR", CompletionKind::Type).with_priority(35),
        CompletionItem::new("WCHAR", CompletionKind::Type).with_priority(40),
        // Time
        CompletionItem::new("TIME", CompletionKind::Type).with_priority(35),
        CompletionItem::new("LTIME", CompletionKind::Type).with_priority(35),
        CompletionItem::new("DATE", CompletionKind::Type).with_priority(40),
        CompletionItem::new("LDATE", CompletionKind::Type).with_priority(40),
        CompletionItem::new("TIME_OF_DAY", CompletionKind::Type).with_priority(40),
        CompletionItem::new("LTIME_OF_DAY", CompletionKind::Type).with_priority(40),
        CompletionItem::new("DATE_AND_TIME", CompletionKind::Type).with_priority(40),
        CompletionItem::new("LDATE_AND_TIME", CompletionKind::Type).with_priority(40),
        // Generic
        CompletionItem::new("ANY", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_DERIVED", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_ELEMENTARY", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_MAGNITUDE", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_INT", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_UNSIGNED", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_SIGNED", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_REAL", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_NUM", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_DURATION", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_BIT", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_CHARS", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_STRING", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_CHAR", CompletionKind::Type).with_priority(45),
        CompletionItem::new("ANY_DATE", CompletionKind::Type).with_priority(45),
        // Compound
        CompletionItem::new("ARRAY", CompletionKind::Keyword)
            .with_insert_text("ARRAY[${1:0}..${2:10}] OF ${3:INT}")
            .with_priority(30),
        CompletionItem::new("POINTER TO", CompletionKind::Keyword).with_priority(40),
        CompletionItem::new("REF_TO", CompletionKind::Keyword).with_priority(40),
    ]
}

fn var_block_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("CONSTANT", CompletionKind::Keyword).with_priority(50),
        CompletionItem::new("RETAIN", CompletionKind::Keyword).with_priority(50),
        CompletionItem::new("PERSISTENT", CompletionKind::Keyword).with_priority(50),
    ]
}

fn var_block_snippets() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("VAR", CompletionKind::Keyword)
            .with_insert_text("VAR\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_INPUT", CompletionKind::Keyword)
            .with_insert_text("VAR_INPUT\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_OUTPUT", CompletionKind::Keyword)
            .with_insert_text("VAR_OUTPUT\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_IN_OUT", CompletionKind::Keyword)
            .with_insert_text("VAR_IN_OUT\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_TEMP", CompletionKind::Keyword)
            .with_insert_text("VAR_TEMP\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_STAT", CompletionKind::Keyword)
            .with_insert_text("VAR_STAT\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_GLOBAL", CompletionKind::Keyword)
            .with_insert_text("VAR_GLOBAL\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_EXTERNAL", CompletionKind::Keyword)
            .with_insert_text("VAR_EXTERNAL\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_ACCESS", CompletionKind::Keyword)
            .with_insert_text("VAR_ACCESS\n\t$0\nEND_VAR")
            .with_priority(12),
        CompletionItem::new("VAR_CONFIG", CompletionKind::Keyword)
            .with_insert_text("VAR_CONFIG\n\t$0\nEND_VAR")
            .with_priority(12),
    ]
}

fn expression_keywords() -> Vec<CompletionItem> {
    vec![
        CompletionItem::new("TRUE", CompletionKind::Keyword).with_priority(30),
        CompletionItem::new("FALSE", CompletionKind::Keyword).with_priority(30),
        CompletionItem::new("AND", CompletionKind::Keyword).with_priority(40),
        CompletionItem::new("OR", CompletionKind::Keyword).with_priority(40),
        CompletionItem::new("XOR", CompletionKind::Keyword).with_priority(40),
        CompletionItem::new("NOT", CompletionKind::Keyword).with_priority(40),
        CompletionItem::new("MOD", CompletionKind::Keyword).with_priority(40),
        CompletionItem::new("THIS", CompletionKind::Keyword).with_priority(50),
        CompletionItem::new("SUPER", CompletionKind::Keyword).with_priority(50),
    ]
}

fn symbols_in_scope(
    filter: &SymbolFilter<'_>,
    scope_id: ScopeId,
    stdlib_filter: &StdlibFilter,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen: FxHashSet<String> = FxHashSet::default();
    for symbol in filter.scope_symbols(scope_id) {
        if matches!(symbol.kind, SymbolKind::FunctionBlock)
            && stdlib_docs::is_standard_fb_name(symbol.name.as_str())
            && !stdlib_filter.allows_function_block(symbol.name.as_str())
        {
            continue;
        }
        let kind = match symbol.kind {
            SymbolKind::Variable { .. } => CompletionKind::Variable,
            SymbolKind::Constant => CompletionKind::Constant,
            SymbolKind::Function { .. } => CompletionKind::Function,
            SymbolKind::FunctionBlock => CompletionKind::FunctionBlock,
            SymbolKind::Class => CompletionKind::Type,
            SymbolKind::Method { .. } => CompletionKind::Method,
            SymbolKind::Property { .. } => CompletionKind::Property,
            SymbolKind::Interface | SymbolKind::Type => CompletionKind::Type,
            SymbolKind::EnumValue { .. } => CompletionKind::EnumValue,
            SymbolKind::Program
            | SymbolKind::ProgramInstance
            | SymbolKind::Parameter { .. }
            | SymbolKind::Namespace
            | SymbolKind::Configuration
            | SymbolKind::Resource
            | SymbolKind::Task => CompletionKind::Variable,
        };

        let mut item = CompletionItem::new(symbol.name.clone(), kind);
        if let Some(type_name) = TypeId::builtin_name(symbol.type_id) {
            item = item.with_detail(type_name);
        }
        item = attach_symbol_docs(item, symbol, filter, Some(scope_id), stdlib_filter);
        seen.insert(symbol.name.to_ascii_uppercase());
        items.push(item);
    }

    items.extend(using_scope_symbol_completions(
        filter.symbols(),
        scope_id,
        &seen,
        stdlib_filter,
    ));
    items
}

fn type_symbols(filter: &SymbolFilter<'_>) -> Vec<CompletionItem> {
    filter
        .type_symbols()
        .map(|symbol| CompletionItem::new(symbol.name.clone(), CompletionKind::Type))
        .collect()
}

fn member_access_completions(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
    root: &SyntaxNode,
    symbols: &SymbolTable,
    scope_id: ScopeId,
    stdlib_filter: &StdlibFilter,
) -> Vec<CompletionItem> {
    let Some(base_type) = member_access_base_type(db, file_id, position, root, symbols) else {
        return Vec::new();
    };
    let base_type = symbols.resolve_alias_type(base_type);

    match symbols.type_by_id(base_type) {
        Some(Type::Struct { fields, .. }) => fields
            .iter()
            .map(|field| {
                let mut item = CompletionItem::new(field.name.clone(), CompletionKind::Variable)
                    .with_priority(10);
                if let Some(detail) = type_detail(symbols, field.type_id) {
                    item = item.with_detail(detail);
                }
                item
            })
            .collect(),
        Some(Type::Union { variants, .. }) => variants
            .iter()
            .map(|variant| {
                let mut item = CompletionItem::new(variant.name.clone(), CompletionKind::Variable)
                    .with_priority(10);
                if let Some(detail) = type_detail(symbols, variant.type_id) {
                    item = item.with_detail(detail);
                }
                item
            })
            .collect(),
        Some(Type::Enum { values, .. }) => values
            .iter()
            .map(|(name, _)| {
                CompletionItem::new(name.clone(), CompletionKind::EnumValue).with_priority(10)
            })
            .collect(),
        Some(Type::FunctionBlock { .. } | Type::Class { .. } | Type::Interface { .. }) => {
            member_symbols_for_type(symbols, base_type, scope_id, stdlib_filter)
        }
        _ => Vec::new(),
    }
}

fn member_access_base_type(
    db: &Database,
    file_id: trust_hir::db::FileId,
    position: TextSize,
    root: &SyntaxNode,
    symbols: &SymbolTable,
) -> Option<TypeId> {
    let token = find_token_at_position(root, position)?;
    let dot = if token.kind() == SyntaxKind::Dot {
        Some(token)
    } else {
        previous_non_trivia_token(&token).filter(|t| t.kind() == SyntaxKind::Dot)
    }?;

    if let Some(field_expr) = dot
        .parent_ancestors()
        .find(|n| n.kind() == SyntaxKind::FieldExpr)
    {
        if let Some(base_expr) = field_expr.children().next() {
            if let Some(base_type) =
                base_type_from_expr_node(db, file_id, symbols, root, &base_expr)
            {
                return Some(base_type);
            }
        }
    }

    let offset = u32::from(dot.text_range().start());
    let offset = offset.saturating_sub(1);
    let expr_id = db.expr_id_at_offset(file_id, offset)?;
    Some(db.type_of(file_id, expr_id))
}

fn base_type_from_expr_node(
    db: &Database,
    file_id: trust_hir::db::FileId,
    symbols: &SymbolTable,
    root: &SyntaxNode,
    node: &SyntaxNode,
) -> Option<TypeId> {
    if node.kind() == SyntaxKind::NameRef {
        let ident = node
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == SyntaxKind::Ident)?;
        let scope_id = scope_at_position(symbols, root, node.text_range().start());
        if let Some(symbol_id) = symbols.resolve(ident.text(), scope_id) {
            if let Some(symbol) = symbols.get(symbol_id) {
                return Some(symbol.type_id);
            }
        }
    }

    let offset = u32::from(node.text_range().start());
    let expr_id = db.expr_id_at_offset(file_id, offset)?;
    Some(db.type_of(file_id, expr_id))
}

fn member_symbols_for_type(
    symbols: &SymbolTable,
    type_id: TypeId,
    scope_id: ScopeId,
    stdlib_filter: &StdlibFilter,
) -> Vec<CompletionItem> {
    let filter = SymbolFilter::new(symbols);
    let Some(owner_id) = filter.owner_for_type(type_id) else {
        return Vec::new();
    };
    let current_owner = current_owner_for_scope(symbols, scope_id);
    let current_namespace = namespace_path_for_scope(symbols, scope_id);

    filter
        .members_in_hierarchy(owner_id, |symbol| is_member_symbol_kind(&symbol.kind))
        .into_iter()
        .filter(|symbol| {
            is_member_visible(symbols, symbol, owner_id, current_owner, &current_namespace)
        })
        .filter_map(|symbol| completion_item_for_symbol(symbol, symbols, stdlib_filter))
        .collect()
}

fn current_owner_for_scope(symbols: &SymbolTable, scope_id: ScopeId) -> Option<SymbolId> {
    let mut current = Some(scope_id);
    while let Some(scope_id) = current {
        let Some(scope) = symbols.get_scope(scope_id) else {
            break;
        };
        if let Some(owner_id) = scope.owner {
            if let Some(symbol) = symbols.get(owner_id) {
                match symbol.kind {
                    SymbolKind::Class | SymbolKind::FunctionBlock => return Some(owner_id),
                    SymbolKind::Method { .. } | SymbolKind::Property { .. } => {
                        if let Some(parent) = symbol.parent {
                            return Some(parent);
                        }
                        return Some(owner_id);
                    }
                    _ => {}
                }
            }
        }
        current = scope.parent;
    }
    None
}

fn namespace_path_for_scope(symbols: &SymbolTable, scope_id: ScopeId) -> Vec<SmolStr> {
    let mut current = Some(scope_id);
    while let Some(scope_id) = current {
        let Some(scope) = symbols.get_scope(scope_id) else {
            break;
        };
        if let Some(owner_id) = scope.owner {
            if let Some(symbol) = symbols.get(owner_id) {
                return namespace_path_for_symbol(symbols, symbol);
            }
        }
        current = scope.parent;
    }
    Vec::new()
}

fn is_member_visible(
    symbols: &SymbolTable,
    member: &trust_hir::symbols::Symbol,
    owner_id: SymbolId,
    current_owner: Option<SymbolId>,
    current_namespace: &[SmolStr],
) -> bool {
    match member.visibility {
        Visibility::Public => true,
        Visibility::Private => current_owner == Some(owner_id),
        Visibility::Protected => {
            current_owner.is_some_and(|current| is_same_or_derived(symbols, current, owner_id))
        }
        Visibility::Internal => {
            let owner_namespace = symbols
                .get(owner_id)
                .map(|symbol| namespace_path_for_symbol(symbols, symbol))
                .unwrap_or_default();
            owner_namespace == current_namespace
        }
    }
}

fn is_same_or_derived(symbols: &SymbolTable, derived_id: SymbolId, base_id: SymbolId) -> bool {
    if derived_id == base_id {
        return true;
    }
    let mut visited: FxHashSet<SymbolId> = FxHashSet::default();
    let mut current = symbols
        .extends_name(derived_id)
        .and_then(|name| symbols.resolve_by_name(name.as_str()));
    while let Some(symbol_id) = current {
        if !visited.insert(symbol_id) {
            break;
        }
        if symbol_id == base_id {
            return true;
        }
        current = symbols
            .extends_name(symbol_id)
            .and_then(|name| symbols.resolve_by_name(name.as_str()));
    }
    false
}

fn completion_item_for_symbol(
    symbol: &trust_hir::symbols::Symbol,
    symbols: &SymbolTable,
    stdlib_filter: &StdlibFilter,
) -> Option<CompletionItem> {
    if matches!(symbol.kind, SymbolKind::FunctionBlock)
        && stdlib_docs::is_standard_fb_name(symbol.name.as_str())
        && !stdlib_filter.allows_function_block(symbol.name.as_str())
    {
        return None;
    }
    let kind = match symbol.kind {
        SymbolKind::Variable { .. } => CompletionKind::Variable,
        SymbolKind::Constant => CompletionKind::Constant,
        SymbolKind::Function { .. } => CompletionKind::Function,
        SymbolKind::Method { .. } => CompletionKind::Method,
        SymbolKind::Property { .. } => CompletionKind::Property,
        _ => return None,
    };
    let mut item = CompletionItem::new(symbol.name.clone(), kind).with_priority(10);
    if let Some(detail) = type_detail(symbols, symbol.type_id) {
        item = item.with_detail(detail);
    }
    item = attach_symbol_docs_simple(item, symbol, symbols, stdlib_filter);
    Some(item)
}

fn attach_symbol_docs_simple(
    mut item: CompletionItem,
    symbol: &trust_hir::symbols::Symbol,
    symbols: &SymbolTable,
    stdlib_filter: &StdlibFilter,
) -> CompletionItem {
    let mut docs = Vec::new();
    if let Some(existing) = &item.documentation {
        docs.push(existing.to_string());
    }
    if let Some(doc) = &symbol.doc {
        docs.push(doc.to_string());
    } else if stdlib_filter.allows_function_block(symbol.name.as_str()) {
        if let Some(std_doc) = stdlib_docs::standard_fb_doc(symbol.name.as_str()) {
            docs.push(std_doc.to_string());
        }
    }
    if let Some(namespace) = namespace_string_for_symbol(symbols, symbol) {
        docs.push(format!("Namespace: {namespace}"));
    }
    if let Some(visibility) = visibility_label(symbol.visibility) {
        docs.push(format!("Visibility: {visibility}"));
    }
    if let Some(mods) = modifiers_label(symbol.modifiers) {
        docs.push(format!("Modifiers: {mods}"));
    }
    if !docs.is_empty() {
        item.documentation = Some(SmolStr::new(docs.join("\n\n")));
    }
    item
}

fn attach_symbol_docs(
    mut item: CompletionItem,
    symbol: &trust_hir::symbols::Symbol,
    filter: &SymbolFilter<'_>,
    scope_id: Option<ScopeId>,
    stdlib_filter: &StdlibFilter,
) -> CompletionItem {
    let mut docs = Vec::new();
    if let Some(doc) = &symbol.doc {
        docs.push(doc.to_string());
    } else if stdlib_filter.allows_function_block(symbol.name.as_str()) {
        if let Some(std_doc) = stdlib_docs::standard_fb_doc(symbol.name.as_str()) {
            docs.push(std_doc.to_string());
        }
    }
    if let Some(namespace) = namespace_string_for_symbol(filter.symbols(), symbol) {
        docs.push(format!("Namespace: {namespace}"));
    }
    if let Some(scope_id) = scope_id {
        if let Some(using_path) =
            using_path_for_symbol(filter.symbols(), scope_id, symbol.name.as_str(), symbol.id)
        {
            let path = join_namespace_path(&using_path);
            docs.push(format!("USING {path}"));
        }
    }
    if let Some(visibility) = visibility_label(symbol.visibility) {
        docs.push(format!("Visibility: {visibility}"));
    }
    if let Some(mods) = modifiers_label(symbol.modifiers) {
        docs.push(format!("Modifiers: {mods}"));
    }
    if !docs.is_empty() {
        item.documentation = Some(SmolStr::new(docs.join("\n\n")));
    }
    item
}

fn namespace_string_for_symbol(
    symbols: &SymbolTable,
    symbol: &trust_hir::symbols::Symbol,
) -> Option<String> {
    let parts = namespace_path_for_symbol(symbols, symbol);
    if parts.is_empty() {
        return None;
    }
    Some(join_namespace_path(&parts))
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

fn visibility_label(visibility: trust_hir::symbols::Visibility) -> Option<&'static str> {
    match visibility {
        trust_hir::symbols::Visibility::Public => None,
        trust_hir::symbols::Visibility::Private => Some("PRIVATE"),
        trust_hir::symbols::Visibility::Protected => Some("PROTECTED"),
        trust_hir::symbols::Visibility::Internal => Some("INTERNAL"),
    }
}

fn modifiers_label(modifiers: trust_hir::symbols::SymbolModifiers) -> Option<String> {
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

fn using_scope_symbol_completions(
    symbols: &SymbolTable,
    scope_id: ScopeId,
    seen: &FxHashSet<String>,
    stdlib_filter: &StdlibFilter,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut known = seen.clone();
    let mut current = Some(scope_id);
    while let Some(scope_id) = current {
        let Some(scope) = symbols.get_scope(scope_id) else {
            break;
        };
        for using in &scope.using_directives {
            let Some(namespace_id) = symbols.resolve_qualified(&using.path) else {
                continue;
            };
            for symbol in symbols
                .iter()
                .filter(|sym| sym.parent == Some(namespace_id))
            {
                if matches!(symbol.kind, SymbolKind::Namespace) {
                    continue;
                }
                if matches!(symbol.kind, SymbolKind::FunctionBlock)
                    && stdlib_docs::is_standard_fb_name(symbol.name.as_str())
                    && !stdlib_filter.allows_function_block(symbol.name.as_str())
                {
                    continue;
                }
                let key = symbol.name.to_ascii_uppercase();
                if !known.insert(key) {
                    continue;
                }

                let kind = match symbol.kind {
                    SymbolKind::Variable { .. } => CompletionKind::Variable,
                    SymbolKind::Constant => CompletionKind::Constant,
                    SymbolKind::Function { .. } => CompletionKind::Function,
                    SymbolKind::FunctionBlock => CompletionKind::FunctionBlock,
                    SymbolKind::Class => CompletionKind::Type,
                    SymbolKind::Method { .. } => CompletionKind::Method,
                    SymbolKind::Property { .. } => CompletionKind::Property,
                    SymbolKind::Interface | SymbolKind::Type => CompletionKind::Type,
                    SymbolKind::EnumValue { .. } => CompletionKind::EnumValue,
                    SymbolKind::Program
                    | SymbolKind::ProgramInstance
                    | SymbolKind::Parameter { .. }
                    | SymbolKind::Namespace
                    | SymbolKind::Configuration
                    | SymbolKind::Resource
                    | SymbolKind::Task => CompletionKind::Variable,
                };

                let mut item = CompletionItem::new(symbol.name.clone(), kind);
                if let Some(type_name) = TypeId::builtin_name(symbol.type_id) {
                    item = item.with_detail(type_name);
                }
                let path = join_namespace_path(&using.path);
                item.documentation = Some(SmolStr::new(format!("USING {path}")));
                item = attach_symbol_docs_simple(item, symbol, symbols, stdlib_filter);
                items.push(item);
            }
        }
        current = scope.parent;
    }
    items
}

fn standard_function_completions(stdlib_filter: &StdlibFilter) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for entry in stdlib_docs::standard_function_entries() {
        if !stdlib_filter.allows_function(entry.name.as_str()) {
            continue;
        }
        let mut item =
            CompletionItem::new(entry.name.clone(), CompletionKind::Function).with_priority(120);
        item.detail = Some(SmolStr::new("standard function"));
        item.documentation = Some(SmolStr::new(entry.doc));
        item.insert_text = Some(SmolStr::new(format!("{}($0)", entry.name)));
        items.push(item);
    }
    items
}

fn typed_literal_completions() -> Vec<CompletionItem> {
    typed_literal_templates()
        .iter()
        .map(|template| {
            let prefix = template.primary_prefix();
            let label = format!("{prefix}#{}", template.value_label);
            let mut item = CompletionItem::new(label, CompletionKind::Snippet)
                .with_insert_text(format!("{prefix}#{}", template.value_snippet))
                .with_priority(15);
            if let Some(doc) = stdlib_docs::typed_literal_doc(prefix) {
                item.documentation = Some(SmolStr::new(doc));
            }
            item
        })
        .collect()
}

fn typed_literal_completions_with_context(
    context: Option<&TypedLiteralContext>,
) -> Vec<CompletionItem> {
    let Some(context) = context else {
        return typed_literal_completions();
    };
    typed_literal_completions_for_context(context)
}

#[derive(Debug, Clone)]
struct TypedLiteralTemplate {
    prefixes: &'static [&'static str],
    value_label: &'static str,
    value_snippet: &'static str,
}

impl TypedLiteralTemplate {
    fn primary_prefix(&self) -> &'static str {
        self.prefixes[0]
    }

    fn matches_prefix(&self, prefix: &str) -> bool {
        self.prefixes
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(prefix))
    }
}

fn typed_literal_templates() -> &'static [TypedLiteralTemplate] {
    &[
        TypedLiteralTemplate {
            prefixes: &["T", "TIME", "LT", "LTIME"],
            value_label: "1s",
            value_snippet: "${1:1s}",
        },
        TypedLiteralTemplate {
            prefixes: &["DATE", "D", "LDATE", "LD"],
            value_label: "2024-01-15",
            value_snippet: "${1:2024-01-15}",
        },
        TypedLiteralTemplate {
            prefixes: &["TOD", "TIME_OF_DAY", "LTOD", "LTIME_OF_DAY"],
            value_label: "14:30:00",
            value_snippet: "${1:14:30:00}",
        },
        TypedLiteralTemplate {
            prefixes: &["DT", "DATE_AND_TIME", "LDT", "LDATE_AND_TIME"],
            value_label: "2024-01-15-14:30:00",
            value_snippet: "${1:2024-01-15-14:30:00}",
        },
        TypedLiteralTemplate {
            prefixes: &["BOOL"],
            value_label: "TRUE",
            value_snippet: "${1:TRUE}",
        },
        TypedLiteralTemplate {
            prefixes: &["SINT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["INT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["DINT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["LINT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["USINT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["UINT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["UDINT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["ULINT"],
            value_label: "0",
            value_snippet: "${1:0}",
        },
        TypedLiteralTemplate {
            prefixes: &["REAL"],
            value_label: "0.0",
            value_snippet: "${1:0.0}",
        },
        TypedLiteralTemplate {
            prefixes: &["LREAL"],
            value_label: "0.0",
            value_snippet: "${1:0.0}",
        },
        TypedLiteralTemplate {
            prefixes: &["BYTE"],
            value_label: "16#FF",
            value_snippet: "${1:16#FF}",
        },
        TypedLiteralTemplate {
            prefixes: &["WORD"],
            value_label: "16#FFFF",
            value_snippet: "${1:16#FFFF}",
        },
        TypedLiteralTemplate {
            prefixes: &["DWORD"],
            value_label: "16#FFFF_FFFF",
            value_snippet: "${1:16#FFFF_FFFF}",
        },
        TypedLiteralTemplate {
            prefixes: &["LWORD"],
            value_label: "16#FFFF_FFFF_FFFF_FFFF",
            value_snippet: "${1:16#FFFF_FFFF_FFFF_FFFF}",
        },
        TypedLiteralTemplate {
            prefixes: &["STRING"],
            value_label: "'text'",
            value_snippet: "'${1:text}'",
        },
        TypedLiteralTemplate {
            prefixes: &["WSTRING"],
            value_label: "\"text\"",
            value_snippet: "\"${1:text}\"",
        },
        TypedLiteralTemplate {
            prefixes: &["CHAR"],
            value_label: "'A'",
            value_snippet: "'${1:A}'",
        },
        TypedLiteralTemplate {
            prefixes: &["WCHAR"],
            value_label: "\"A\"",
            value_snippet: "\"${1:A}\"",
        },
    ]
}

#[derive(Debug, Clone)]
struct TypedLiteralContext {
    prefix: SmolStr,
    prefix_text: SmolStr,
    value_range: TextRange,
}

fn typed_literal_completion_context(
    context: &IdeContext<'_>,
    position: TextSize,
) -> Option<TypedLiteralContext> {
    let token = find_token_at_position(&context.root, position)?;
    let mut prefix_token: Option<SyntaxToken> = None;
    let mut value_range: Option<TextRange> = None;

    if token.kind() == SyntaxKind::TypedLiteralPrefix {
        prefix_token = Some(token.clone());
        value_range = Some(TextRange::new(
            token.text_range().end(),
            token.text_range().end(),
        ));
    } else if let Some(prev) = previous_non_trivia_token(&token) {
        if prev.kind() == SyntaxKind::TypedLiteralPrefix {
            prefix_token = Some(prev);
            if is_typed_literal_value_token(token.kind()) {
                value_range = Some(token.text_range());
            } else {
                value_range = Some(TextRange::new(position, position));
            }
        }
    }

    if let Some(prefix_token) = prefix_token {
        let prefix_text = prefix_token.text().trim_end_matches('#');
        return Some(TypedLiteralContext {
            prefix: SmolStr::new(prefix_text),
            prefix_text: SmolStr::new(prefix_text),
            value_range: value_range?,
        });
    }

    if token.text().contains('#') {
        let text = token.text();
        if let Some(hash_idx) = text.find('#') {
            let prefix_text = &text[..hash_idx];
            let start = token.text_range().start() + TextSize::from((hash_idx + 1) as u32);
            return Some(TypedLiteralContext {
                prefix: SmolStr::new(prefix_text),
                prefix_text: SmolStr::new(prefix_text),
                value_range: TextRange::new(start, token.text_range().end()),
            });
        }
    }

    None
}

fn typed_literal_completions_for_context(context: &TypedLiteralContext) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for template in typed_literal_templates() {
        if !template.matches_prefix(context.prefix.as_str()) {
            continue;
        }
        let label = format!("{}#{}", context.prefix_text, template.value_label);
        let mut item = CompletionItem::new(label, CompletionKind::Snippet)
            .with_text_edit(CompletionTextEdit {
                range: context.value_range,
                new_text: SmolStr::new(template.value_snippet),
            })
            .with_priority(15);
        if let Some(doc) = stdlib_docs::typed_literal_doc(context.prefix.as_str()) {
            item.documentation = Some(SmolStr::new(doc));
        }
        items.push(item);
    }
    items
}

fn is_typed_literal_value_token(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::IntLiteral
            | SyntaxKind::RealLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::WideStringLiteral
            | SyntaxKind::TimeLiteral
            | SyntaxKind::DateLiteral
            | SyntaxKind::TimeOfDayLiteral
            | SyntaxKind::DateAndTimeLiteral
            | SyntaxKind::KwTrue
            | SyntaxKind::KwFalse
            | SyntaxKind::Ident
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use trust_hir::db::{Database, FileId, SourceDatabase};

    #[test]
    fn test_top_level_keywords() {
        let items = top_level_keywords();
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "FUNCTION_BLOCK"));
    }

    #[test]
    fn test_type_keywords() {
        let items = type_keywords();
        assert!(items.iter().any(|i| i.label == "INT"));
        assert!(items.iter().any(|i| i.label == "BOOL"));
    }

    #[test]
    fn test_parameter_name_completion_in_call() {
        let source = r#"
FUNCTION Add : INT
VAR_INPUT
    A : INT;
    B : INT;
END_VAR
    Add := A + B;
END_FUNCTION

PROGRAM Main
VAR
    result : INT;
END_VAR
    result := Add(|);
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        assert!(items
            .iter()
            .any(|item| item.label.eq_ignore_ascii_case("A")));
        assert!(items
            .iter()
            .any(|item| item.label.eq_ignore_ascii_case("B")));
        let a_item = items
            .iter()
            .find(|item| item.label.eq_ignore_ascii_case("A"))
            .expect("A completion");
        let insert = a_item.insert_text.as_ref().expect("insert text");
        assert!(insert.contains("A"));
        assert!(insert.contains(":="));
    }

    #[test]
    fn test_parameter_name_completion_skips_used_formal() {
        let source = r#"
FUNCTION Add : INT
VAR_INPUT
    A : INT;
    B : INT;
END_VAR
    Add := A + B;
END_FUNCTION

PROGRAM Main
VAR
    result : INT;
END_VAR
    result := Add(A := 1, |);
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        assert!(!items
            .iter()
            .any(|item| item.label.eq_ignore_ascii_case("A")));
        assert!(items
            .iter()
            .any(|item| item.label.eq_ignore_ascii_case("B")));
    }

    #[test]
    fn test_standard_function_completion() {
        let source = r#"
PROGRAM Main
VAR
    x : INT;
END_VAR
    x := |;
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        let abs_item = items
            .iter()
            .find(|item| item.label.eq_ignore_ascii_case("ABS"))
            .expect("ABS completion");
        assert!(abs_item
            .documentation
            .as_ref()
            .map(|doc| doc.contains("IEC 61131-3"))
            .unwrap_or(false));
    }

    #[test]
    fn test_typed_literal_completion() {
        let source = r#"
PROGRAM Main
VAR
    x : TIME;
END_VAR
    x := |;
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        assert!(items.iter().any(|item| item.label == "T#1s"));
        assert!(items.iter().any(|item| item.label == "DATE#2024-01-15"));
    }

    #[test]
    fn test_typed_literal_completion_after_prefix() {
        let source = r#"
PROGRAM Main
VAR
    x : TIME;
END_VAR
    x := T#|;
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        let item = items
            .iter()
            .find(|item| item.label == "T#1s")
            .expect("typed literal snippet");
        let edit = item.text_edit.as_ref().expect("text edit");
        assert_eq!(edit.new_text.as_str(), "${1:1s}");
        assert_eq!(edit.range.start(), TextSize::from(cursor as u32));
        assert_eq!(edit.range.end(), TextSize::from(cursor as u32));
    }

    #[test]
    fn test_member_completion_respects_visibility() {
        let source = r#"
INTERFACE ICounter
    METHOD Next : DINT
    END_METHOD
    PROPERTY Value : DINT
        GET
        END_GET
    END_PROPERTY
END_INTERFACE

FUNCTION_BLOCK CounterFb IMPLEMENTS ICounter
VAR
    x : DINT;
END_VAR

METHOD PUBLIC Next : DINT
    x := x + 1;
    Next := x;
END_METHOD

PUBLIC PROPERTY Value : DINT
    GET
        Value := x;
    END_GET
END_PROPERTY
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    counter : CounterFb;
END_VAR
    counter.|
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        assert!(items.iter().any(|item| item.label == "Next"));
        assert!(items.iter().any(|item| item.label == "Value"));
        assert!(!items.iter().any(|item| item.label == "x"));
    }

    #[test]
    fn test_completion_recovery_in_statement_context_keeps_scope_symbols() {
        let source = r#"
PROGRAM PlantProgram
VAR
    Pump : FB_Pump;
    Cmd : ST_PumpCommand;
    Status : ST_PumpStatus;
    HaltReq : BOOL;
END_VAR

Sta|
Pump(Command := Cmd);
Status := Pump.Status;
HaltReq := FALSE;
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        assert!(
            items
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case("Status")),
            "completion should include local variable Status in recovered statement context"
        );
        assert!(
            items
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case("Cmd")),
            "completion should include local variable Cmd in recovered statement context"
        );
        assert!(
            items
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case("Pump")),
            "completion should include local variable Pump in recovered statement context"
        );
        assert!(
            items
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case("HaltReq")),
            "completion should include local variable HaltReq in recovered statement context"
        );
    }

    #[test]
    fn test_using_namespace_completion_info() {
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
    x := |;
END_PROGRAM
"#;
        let cursor = source.find('|').expect("cursor");
        let mut cleaned = source.to_string();
        cleaned.remove(cursor);

        let mut db = Database::new();
        let file_id = FileId(0);
        db.set_source_text(file_id, cleaned);

        let items = complete(&db, file_id, TextSize::from(cursor as u32));
        let foo = items
            .iter()
            .find(|item| item.label.eq_ignore_ascii_case("Foo"))
            .expect("Foo completion");
        assert!(foo
            .documentation
            .as_ref()
            .map(|doc| doc.contains("USING Lib"))
            .unwrap_or(false));
    }
}
