//! Signature help for Structured Text calls.
//!
//! This module provides signature information for call expressions.

use rustc_hash::FxHashSet;
use smol_str::SmolStr;
use text_size::{TextRange, TextSize};

use trust_hir::db::{FileId, SemanticDatabase};
use trust_hir::symbols::{ParamDirection, Symbol, SymbolKind, SymbolTable};
use trust_hir::{Database, SourceDatabase, TypeId};
use trust_syntax::parser::parse;
use trust_syntax::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::util::{
    name_from_name_node, name_from_name_ref, resolve_target_at_position_with_context,
    scope_at_position, ResolvedTarget,
};

/// Signature help result for a call site.
#[derive(Debug, Clone)]
pub struct SignatureHelpResult {
    /// All available signatures (usually a single entry).
    pub signatures: Vec<Signature>,
    /// The active signature index.
    pub active_signature: usize,
    /// The active parameter index.
    pub active_parameter: usize,
}

/// Parameter info for call signature metadata.
#[derive(Debug, Clone)]
pub struct CallSignatureParam {
    /// Parameter name.
    pub name: SmolStr,
    /// Parameter direction.
    pub direction: ParamDirection,
}

/// Signature metadata for call transformation helpers.
#[derive(Debug, Clone)]
pub struct CallSignatureInfo {
    /// Callable name.
    pub name: SmolStr,
    /// Parameters in call order.
    pub params: Vec<CallSignatureParam>,
}

/// A callable signature.
#[derive(Debug, Clone)]
pub struct Signature {
    /// Display label for the signature.
    pub label: String,
    /// Parameter metadata for the signature.
    pub parameters: Vec<SignatureParameter>,
}

/// A single parameter in a signature.
#[derive(Debug, Clone)]
pub struct SignatureParameter {
    /// Display label for the parameter.
    pub label: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ParamData {
    pub(crate) name: SmolStr,
    pub(crate) type_id: TypeId,
    pub(crate) direction: ParamDirection,
}

#[derive(Debug, Clone)]
pub(crate) struct SignatureInfo {
    pub(crate) name: SmolStr,
    pub(crate) params: Vec<ParamData>,
    pub(crate) return_type: Option<TypeId>,
}

#[derive(Debug, Clone)]
struct ArgInfo {
    name: Option<SmolStr>,
    range: TextRange,
}

pub(crate) struct CallSignatureContext {
    pub(crate) signature: SignatureInfo,
    pub(crate) used_params: FxHashSet<SmolStr>,
}

pub(crate) fn signature_for_call_expr(
    db: &Database,
    file_id: FileId,
    source: &str,
    root: &SyntaxNode,
    call_expr: &SyntaxNode,
) -> Option<SignatureInfo> {
    let arg_list = call_expr
        .children()
        .find(|child| child.kind() == SyntaxKind::ArgList)?;
    let callee = call_expr
        .children()
        .find(|child| child.kind() != SyntaxKind::ArgList)?;

    let symbols = db.file_symbols_with_project(file_id);
    let callee_offset = callee_name_offset(&callee)?;
    let target =
        resolve_target_at_position_with_context(db, file_id, callee_offset, source, root, &symbols);

    let signature = match target {
        Some(ResolvedTarget::Symbol(symbol_id)) => {
            let symbol = symbols.get(symbol_id)?;
            signature_from_symbol(&symbols, symbol)
                .or_else(|| signature_from_type(&symbols, symbol.type_id))
        }
        Some(ResolvedTarget::Field(_)) => None,
        None => None,
    }
    .or_else(|| {
        if !matches!(callee.kind(), SyntaxKind::NameRef) {
            return None;
        }
        let name = callee_name_text(&callee)?;
        let scope_id = scope_at_position(&symbols, root, callee.text_range().start());
        let symbol_id = symbols
            .resolve(name.as_str(), scope_id)
            .or_else(|| symbols.lookup_any(name.as_str()))?;
        let symbol = symbols.get(symbol_id)?;
        signature_from_symbol(&symbols, symbol)
            .or_else(|| signature_from_type(&symbols, symbol.type_id))
    })
    .or_else(|| {
        let name = callee_name_text(&callee)?;
        let arg_count = collect_call_args(&arg_list).len();
        standard_signature(name.as_str(), arg_count)
    })?;

    Some(signature)
}

pub(crate) fn call_signature_context(
    db: &Database,
    file_id: FileId,
    position: TextSize,
) -> Option<CallSignatureContext> {
    let source = db.source_text(file_id);
    let parsed = parse(&source);
    let root = parsed.syntax();
    let token = find_token_at_position(&root, position)?;
    let call_expr = token
        .parent_ancestors()
        .find(|node| node.kind() == SyntaxKind::CallExpr)?;
    let arg_list = call_expr
        .children()
        .find(|child| child.kind() == SyntaxKind::ArgList)?;

    let signature = signature_for_call_expr(db, file_id, &source, &root, &call_expr)?;
    let args = collect_call_args(&arg_list);
    let arg_types = arg_types_for_args(db, file_id, &args);
    let signature = apply_arg_types(&signature, &arg_types);
    let formal_call = args.iter().any(|arg| arg.name.is_some());
    let signature = if formal_call {
        signature
    } else {
        strip_execution_params(&signature)
    };
    let mut used_params: FxHashSet<SmolStr> = FxHashSet::default();
    for arg in args {
        if let Some(name) = arg.name {
            used_params.insert(SmolStr::new(name.to_ascii_uppercase()));
        }
    }

    Some(CallSignatureContext {
        signature,
        used_params,
    })
}

/// Returns call signature metadata (name + parameters) for the call at position.
pub fn call_signature_info(
    db: &Database,
    file_id: FileId,
    position: TextSize,
) -> Option<CallSignatureInfo> {
    let context = call_signature_context(db, file_id, position)?;
    let params = context
        .signature
        .params
        .iter()
        .map(|param| CallSignatureParam {
            name: param.name.clone(),
            direction: param.direction,
        })
        .collect();
    Some(CallSignatureInfo {
        name: context.signature.name.clone(),
        params,
    })
}

/// Computes signature help information at a given position.
pub fn signature_help(
    db: &Database,
    file_id: FileId,
    position: TextSize,
) -> Option<SignatureHelpResult> {
    let source = db.source_text(file_id);
    let parsed = parse(&source);
    let root = parsed.syntax();
    let token = find_token_at_position(&root, position)?;
    let call_expr = token
        .parent_ancestors()
        .find(|node| node.kind() == SyntaxKind::CallExpr)?;
    let arg_list = call_expr
        .children()
        .find(|child| child.kind() == SyntaxKind::ArgList)?;
    let symbols = db.file_symbols_with_project(file_id);
    let signature = signature_for_call_expr(db, file_id, &source, &root, &call_expr)?;

    let args = collect_call_args(&arg_list);
    let arg_types = arg_types_for_args(db, file_id, &args);
    let signature = apply_arg_types(&signature, &arg_types);
    let formal_call = args.iter().any(|arg| arg.name.is_some());
    let signature = if formal_call {
        signature
    } else {
        strip_execution_params(&signature)
    };
    let active_arg = active_arg_index(&args, &arg_list, position);
    let mut active_param = active_param_index(&args, active_arg, &signature.params);
    if signature.params.is_empty() {
        active_param = 0;
    } else if active_param >= signature.params.len() {
        active_param = signature.params.len() - 1;
    }

    let label = format_signature_label(&symbols, &signature);
    let parameters = signature
        .params
        .iter()
        .map(|param| SignatureParameter {
            label: format_param_label(&symbols, param),
        })
        .collect();

    Some(SignatureHelpResult {
        signatures: vec![Signature { label, parameters }],
        active_signature: 0,
        active_parameter: active_param,
    })
}

fn find_token_at_position(root: &SyntaxNode, position: TextSize) -> Option<SyntaxToken> {
    let token = root.token_at_offset(position);
    token
        .clone()
        .right_biased()
        .or_else(|| token.left_biased())
        .or_else(|| root.last_token())
}

fn collect_call_args(arg_list: &SyntaxNode) -> Vec<ArgInfo> {
    let mut args = Vec::new();
    for arg in arg_list.children().filter(|n| n.kind() == SyntaxKind::Arg) {
        let name = arg
            .children()
            .find(|child| child.kind() == SyntaxKind::Name)
            .and_then(|child| name_from_name_node(&child));
        args.push(ArgInfo {
            name,
            range: arg.text_range(),
        });
    }
    args
}

fn active_arg_index(args: &[ArgInfo], arg_list: &SyntaxNode, position: TextSize) -> usize {
    if let Some((index, _)) = args
        .iter()
        .enumerate()
        .find(|(_, arg)| arg.range.contains(position))
    {
        return index;
    }

    let mut comma_count = 0usize;
    for token in arg_list
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        if token.kind() == SyntaxKind::Comma && token.text_range().end() <= position {
            comma_count += 1;
        }
    }
    comma_count
}

fn active_param_index(args: &[ArgInfo], arg_index: usize, params: &[ParamData]) -> usize {
    if args.is_empty() {
        return 0;
    }
    if arg_index >= args.len() {
        return arg_index;
    }
    let arg = &args[arg_index];
    if let Some(name) = &arg.name {
        if let Some(index) = params
            .iter()
            .position(|param| param.name.eq_ignore_ascii_case(name.as_str()))
        {
            return index;
        }
    }
    arg_index
}

fn arg_types_for_args(db: &Database, file_id: FileId, args: &[ArgInfo]) -> Vec<Option<TypeId>> {
    args.iter()
        .map(|arg| arg_type_at_range(db, file_id, arg.range))
        .collect()
}

fn arg_type_at_range(db: &Database, file_id: FileId, range: TextRange) -> Option<TypeId> {
    let offset = range.start();
    let expr_id = db.expr_id_at_offset(file_id, offset.into())?;
    Some(db.type_of(file_id, expr_id))
}

fn apply_arg_types(signature: &SignatureInfo, arg_types: &[Option<TypeId>]) -> SignatureInfo {
    let mut updated = signature.clone();
    for (idx, param) in updated.params.iter_mut().enumerate() {
        let Some(Some(arg_type)) = arg_types.get(idx) else {
            continue;
        };
        if is_generic_type(param.type_id) {
            param.type_id = *arg_type;
        }
    }

    if let Some(return_type) = updated.return_type {
        if is_generic_type(return_type) {
            if let Some(Some(arg_type)) = arg_types.first() {
                updated.return_type = Some(*arg_type);
            }
        }
    } else if let Some(Some(arg_type)) = arg_types.first() {
        updated.return_type = Some(*arg_type);
    }

    updated
}

fn is_generic_type(type_id: TypeId) -> bool {
    matches!(
        type_id,
        TypeId::ANY
            | TypeId::ANY_DERIVED
            | TypeId::ANY_ELEMENTARY
            | TypeId::ANY_MAGNITUDE
            | TypeId::ANY_INT
            | TypeId::ANY_UNSIGNED
            | TypeId::ANY_SIGNED
            | TypeId::ANY_REAL
            | TypeId::ANY_NUM
            | TypeId::ANY_DURATION
            | TypeId::ANY_BIT
            | TypeId::ANY_CHARS
            | TypeId::ANY_STRING
            | TypeId::ANY_CHAR
            | TypeId::ANY_DATE
    )
}

fn strip_execution_params(signature: &SignatureInfo) -> SignatureInfo {
    let mut filtered = signature.clone();
    filtered
        .params
        .retain(|param| !is_execution_param_name(param.name.as_str()));
    filtered
}

fn is_execution_param_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("EN") || name.eq_ignore_ascii_case("ENO")
}

fn callee_name_offset(node: &SyntaxNode) -> Option<TextSize> {
    match node.kind() {
        SyntaxKind::NameRef => node
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::Ident)
            .map(|token| token.text_range().start()),
        SyntaxKind::FieldExpr => node
            .descendants()
            .filter(|child| child.kind() == SyntaxKind::NameRef)
            .last()
            .and_then(|child| {
                child
                    .descendants_with_tokens()
                    .filter_map(|element| element.into_token())
                    .find(|token| token.kind() == SyntaxKind::Ident)
                    .map(|token| token.text_range().start())
            }),
        _ => None,
    }
}

fn callee_name_text(node: &SyntaxNode) -> Option<SmolStr> {
    match node.kind() {
        SyntaxKind::NameRef => name_from_name_ref(node),
        SyntaxKind::FieldExpr => node
            .descendants()
            .filter(|child| child.kind() == SyntaxKind::NameRef)
            .last()
            .and_then(|child| name_from_name_ref(&child)),
        _ => None,
    }
}

fn signature_from_symbol(symbols: &SymbolTable, symbol: &Symbol) -> Option<SignatureInfo> {
    let params = callable_params(symbols, symbol);
    if params.is_empty() && !symbol.is_callable() {
        return None;
    }

    let return_type = match symbol.kind {
        SymbolKind::Function { return_type, .. } => Some(return_type),
        SymbolKind::Method { return_type, .. } => return_type,
        _ => None,
    };

    Some(SignatureInfo {
        name: symbol.name.clone(),
        params,
        return_type,
    })
}

fn signature_from_type(symbols: &SymbolTable, type_id: TypeId) -> Option<SignatureInfo> {
    let symbol = symbols.iter().find(|sym| {
        sym.type_id == type_id
            && matches!(
                sym.kind,
                SymbolKind::FunctionBlock | SymbolKind::Class | SymbolKind::Interface
            )
    })?;

    let params = callable_params(symbols, symbol);
    Some(SignatureInfo {
        name: symbol.name.clone(),
        params,
        return_type: None,
    })
}

fn callable_params(symbols: &SymbolTable, symbol: &Symbol) -> Vec<ParamData> {
    let mut ids: Vec<_> = match &symbol.kind {
        SymbolKind::Function { parameters, .. } | SymbolKind::Method { parameters, .. } => {
            parameters.clone()
        }
        _ => Vec::new(),
    };

    if ids.is_empty() {
        ids = symbols
            .iter()
            .filter(|sym| {
                sym.parent == Some(symbol.id) && matches!(sym.kind, SymbolKind::Parameter { .. })
            })
            .map(|sym| sym.id)
            .collect();
    }

    ids.sort_by_key(|id| id.0);
    ids.into_iter()
        .filter_map(|id| {
            let sym = symbols.get(id)?;
            match sym.kind {
                SymbolKind::Parameter { direction } => Some(ParamData {
                    name: sym.name.clone(),
                    type_id: sym.type_id,
                    direction,
                }),
                _ => None,
            }
        })
        .collect()
}

fn format_signature_label(symbols: &SymbolTable, signature: &SignatureInfo) -> String {
    let params = signature
        .params
        .iter()
        .map(|param| format_param_label(symbols, param))
        .collect::<Vec<_>>()
        .join(", ");

    let mut label = format!("{}({})", signature.name, params);
    if let Some(return_type) = signature.return_type {
        let ret_name = format_type_name(symbols, return_type);
        label.push_str(&format!(" : {}", ret_name));
    }
    label
}

fn format_param_label(symbols: &SymbolTable, param: &ParamData) -> String {
    let type_name = format_type_name(symbols, param.type_id);
    let mut label = format!("{}: {}", param.name, type_name);
    let suffix = match param.direction {
        ParamDirection::In => None,
        ParamDirection::Out => Some("OUT"),
        ParamDirection::InOut => Some("IN_OUT"),
    };
    if let Some(dir) = suffix {
        label.push_str(&format!(" ({})", dir));
    }
    label
}

fn format_type_name(symbols: &SymbolTable, type_id: TypeId) -> String {
    if let Some(name) = symbols.type_name(type_id) {
        return name.to_string();
    }
    type_id
        .builtin_name()
        .map(|name| name.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn standard_signature(name: &str, arg_count: usize) -> Option<SignatureInfo> {
    let upper = name.to_ascii_uppercase();

    if let Some(signature) = conversion_signature(&upper) {
        return Some(signature);
    }

    let (params, return_type) = match upper.as_str() {
        // Numeric
        "ABS" => (vec![param("IN", TypeId::ANY_NUM)], None),
        "SQRT" | "LN" | "LOG" | "EXP" | "SIN" | "COS" | "TAN" | "ASIN" | "ACOS" | "ATAN" => {
            (vec![param("IN", TypeId::ANY_REAL)], None)
        }
        "ATAN2" => (
            vec![param("Y", TypeId::ANY_REAL), param("X", TypeId::ANY_REAL)],
            None,
        ),
        "ADD" => (variadic_in("IN", arg_count, 2, TypeId::ANY), None),
        "SUB" => (fixed_in("IN", 2, TypeId::ANY), None),
        "MUL" => (variadic_in("IN", arg_count, 2, TypeId::ANY), None),
        "DIV" => (fixed_in("IN", 2, TypeId::ANY), None),
        "MOD" => (fixed_in("IN", 2, TypeId::ANY_NUM), None),
        "EXPT" => (
            vec![
                param("IN1", TypeId::ANY_REAL),
                param("IN2", TypeId::ANY_NUM),
            ],
            None,
        ),
        "MOVE" => (vec![param("IN", TypeId::ANY)], None),

        // Bit
        "SHL" | "SHR" | "ROL" | "ROR" => (
            vec![param("IN", TypeId::ANY_BIT), param("N", TypeId::ANY_INT)],
            None,
        ),
        "AND" | "OR" | "XOR" => (variadic_in("IN", arg_count, 2, TypeId::ANY_BIT), None),
        "NOT" => (vec![param("IN", TypeId::ANY_BIT)], None),

        // Selection
        "SEL" => (
            vec![
                param("G", TypeId::BOOL),
                param("IN0", TypeId::ANY),
                param("IN1", TypeId::ANY),
            ],
            None,
        ),
        "MAX" | "MIN" => (
            variadic_in("IN", arg_count, 2, TypeId::ANY_ELEMENTARY),
            None,
        ),
        "LIMIT" => (
            vec![
                param("MN", TypeId::ANY_ELEMENTARY),
                param("IN", TypeId::ANY_ELEMENTARY),
                param("MX", TypeId::ANY_ELEMENTARY),
            ],
            None,
        ),
        "MUX" => (mux_params(arg_count), None),

        // Comparison
        "GT" | "GE" | "EQ" | "LE" | "LT" => (
            variadic_in("IN", arg_count, 2, TypeId::ANY_ELEMENTARY),
            Some(TypeId::BOOL),
        ),
        "NE" => (
            fixed_in("IN", 2, TypeId::ANY_ELEMENTARY),
            Some(TypeId::BOOL),
        ),

        // String
        "LEN" => (vec![param("IN", TypeId::ANY_STRING)], Some(TypeId::INT)),
        "LEFT" | "RIGHT" => (
            vec![param("IN", TypeId::ANY_STRING), param("L", TypeId::ANY_INT)],
            None,
        ),
        "MID" => (
            vec![
                param("IN", TypeId::ANY_STRING),
                param("L", TypeId::ANY_INT),
                param("P", TypeId::ANY_INT),
            ],
            None,
        ),
        "CONCAT" => (variadic_in("IN", arg_count, 2, TypeId::ANY_STRING), None),
        "INSERT" => (
            vec![
                param("IN1", TypeId::ANY_STRING),
                param("IN2", TypeId::ANY_STRING),
                param("P", TypeId::ANY_INT),
            ],
            None,
        ),
        "DELETE" => (
            vec![
                param("IN", TypeId::ANY_STRING),
                param("L", TypeId::ANY_INT),
                param("P", TypeId::ANY_INT),
            ],
            None,
        ),
        "REPLACE" => (
            vec![
                param("IN1", TypeId::ANY_STRING),
                param("IN2", TypeId::ANY_STRING),
                param("L", TypeId::ANY_INT),
                param("P", TypeId::ANY_INT),
            ],
            None,
        ),
        "FIND" => (
            vec![
                param("IN1", TypeId::ANY_STRING),
                param("IN2", TypeId::ANY_STRING),
            ],
            Some(TypeId::INT),
        ),

        // Time math
        "ADD_TIME" => (time_binary(TypeId::TIME, TypeId::TIME), Some(TypeId::TIME)),
        "ADD_LTIME" => (
            time_binary(TypeId::LTIME, TypeId::LTIME),
            Some(TypeId::LTIME),
        ),
        "ADD_TOD_TIME" => (time_binary(TypeId::TOD, TypeId::TIME), Some(TypeId::TOD)),
        "ADD_LTOD_LTIME" => (time_binary(TypeId::LTOD, TypeId::LTIME), Some(TypeId::LTOD)),
        "ADD_DT_TIME" => (time_binary(TypeId::DT, TypeId::TIME), Some(TypeId::DT)),
        "ADD_LDT_LTIME" => (time_binary(TypeId::LDT, TypeId::LTIME), Some(TypeId::LDT)),
        "SUB_TIME" => (time_binary(TypeId::TIME, TypeId::TIME), Some(TypeId::TIME)),
        "SUB_LTIME" => (
            time_binary(TypeId::LTIME, TypeId::LTIME),
            Some(TypeId::LTIME),
        ),
        "SUB_DATE_DATE" => (time_binary(TypeId::DATE, TypeId::DATE), Some(TypeId::TIME)),
        "SUB_LDATE_LDATE" => (
            time_binary(TypeId::LDATE, TypeId::LDATE),
            Some(TypeId::LTIME),
        ),
        "SUB_TOD_TIME" => (time_binary(TypeId::TOD, TypeId::TIME), Some(TypeId::TOD)),
        "SUB_LTOD_LTIME" => (time_binary(TypeId::LTOD, TypeId::LTIME), Some(TypeId::LTOD)),
        "SUB_TOD_TOD" => (time_binary(TypeId::TOD, TypeId::TOD), Some(TypeId::TIME)),
        "SUB_LTOD_LTOD" => (time_binary(TypeId::LTOD, TypeId::LTOD), Some(TypeId::LTIME)),
        "SUB_DT_TIME" => (time_binary(TypeId::DT, TypeId::TIME), Some(TypeId::DT)),
        "SUB_LDT_LTIME" => (time_binary(TypeId::LDT, TypeId::LTIME), Some(TypeId::LDT)),
        "SUB_DT_DT" => (time_binary(TypeId::DT, TypeId::DT), Some(TypeId::TIME)),
        "SUB_LDT_LDT" => (time_binary(TypeId::LDT, TypeId::LDT), Some(TypeId::LTIME)),
        "MUL_TIME" => (
            vec![param("IN1", TypeId::TIME), param("IN2", TypeId::ANY_NUM)],
            Some(TypeId::TIME),
        ),
        "MUL_LTIME" => (
            vec![param("IN1", TypeId::LTIME), param("IN2", TypeId::ANY_NUM)],
            Some(TypeId::LTIME),
        ),
        "DIV_TIME" => (
            vec![param("IN1", TypeId::TIME), param("IN2", TypeId::ANY_NUM)],
            Some(TypeId::TIME),
        ),
        "DIV_LTIME" => (
            vec![param("IN1", TypeId::LTIME), param("IN2", TypeId::ANY_NUM)],
            Some(TypeId::LTIME),
        ),
        "CONCAT_DATE_TOD" => (
            vec![param("DATE", TypeId::DATE), param("TOD", TypeId::TOD)],
            Some(TypeId::DT),
        ),
        "CONCAT_DATE_LTOD" => (
            vec![param("DATE", TypeId::DATE), param("LTOD", TypeId::LTOD)],
            Some(TypeId::LDT),
        ),
        "CONCAT_DATE" => (
            vec![
                param("YEAR", TypeId::ANY_INT),
                param("MONTH", TypeId::ANY_INT),
                param("DAY", TypeId::ANY_INT),
            ],
            Some(TypeId::DATE),
        ),
        "CONCAT_TOD" => (
            vec![
                param("HOUR", TypeId::ANY_INT),
                param("MINUTE", TypeId::ANY_INT),
                param("SECOND", TypeId::ANY_INT),
                param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::TOD),
        ),
        "CONCAT_LTOD" => (
            vec![
                param("HOUR", TypeId::ANY_INT),
                param("MINUTE", TypeId::ANY_INT),
                param("SECOND", TypeId::ANY_INT),
                param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::LTOD),
        ),
        "CONCAT_DT" => (
            vec![
                param("YEAR", TypeId::ANY_INT),
                param("MONTH", TypeId::ANY_INT),
                param("DAY", TypeId::ANY_INT),
                param("HOUR", TypeId::ANY_INT),
                param("MINUTE", TypeId::ANY_INT),
                param("SECOND", TypeId::ANY_INT),
                param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::DT),
        ),
        "CONCAT_LDT" => (
            vec![
                param("YEAR", TypeId::ANY_INT),
                param("MONTH", TypeId::ANY_INT),
                param("DAY", TypeId::ANY_INT),
                param("HOUR", TypeId::ANY_INT),
                param("MINUTE", TypeId::ANY_INT),
                param("SECOND", TypeId::ANY_INT),
                param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::LDT),
        ),
        "SPLIT_DATE" => (
            vec![
                param("IN", TypeId::DATE),
                out_param("YEAR", TypeId::ANY_INT),
                out_param("MONTH", TypeId::ANY_INT),
                out_param("DAY", TypeId::ANY_INT),
            ],
            Some(TypeId::VOID),
        ),
        "SPLIT_TOD" => (
            vec![
                param("IN", TypeId::TOD),
                out_param("HOUR", TypeId::ANY_INT),
                out_param("MINUTE", TypeId::ANY_INT),
                out_param("SECOND", TypeId::ANY_INT),
                out_param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::VOID),
        ),
        "SPLIT_LTOD" => (
            vec![
                param("IN", TypeId::LTOD),
                out_param("HOUR", TypeId::ANY_INT),
                out_param("MINUTE", TypeId::ANY_INT),
                out_param("SECOND", TypeId::ANY_INT),
                out_param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::VOID),
        ),
        "SPLIT_DT" => (
            vec![
                param("IN", TypeId::DT),
                out_param("YEAR", TypeId::ANY_INT),
                out_param("MONTH", TypeId::ANY_INT),
                out_param("DAY", TypeId::ANY_INT),
                out_param("HOUR", TypeId::ANY_INT),
                out_param("MINUTE", TypeId::ANY_INT),
                out_param("SECOND", TypeId::ANY_INT),
                out_param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::VOID),
        ),
        "SPLIT_LDT" => (
            vec![
                param("IN", TypeId::LDT),
                out_param("YEAR", TypeId::ANY_INT),
                out_param("MONTH", TypeId::ANY_INT),
                out_param("DAY", TypeId::ANY_INT),
                out_param("HOUR", TypeId::ANY_INT),
                out_param("MINUTE", TypeId::ANY_INT),
                out_param("SECOND", TypeId::ANY_INT),
                out_param("MILLISECOND", TypeId::ANY_INT),
            ],
            Some(TypeId::VOID),
        ),
        "DAY_OF_WEEK" => (vec![param("IN", TypeId::DATE)], Some(TypeId::INT)),

        // Special calls
        "REF" => (vec![param("IN", TypeId::ANY)], None),
        "NEW" | "__NEW" => (vec![param("TYPE", TypeId::ANY)], None),
        "__DELETE" => (vec![param("IN", TypeId::ANY)], Some(TypeId::VOID)),
        _ => return None,
    };

    Some(SignatureInfo {
        name: SmolStr::new(name),
        params,
        return_type,
    })
}

fn conversion_signature(name: &str) -> Option<SignatureInfo> {
    let upper = name.to_ascii_uppercase();

    if upper == "TRUNC" {
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY_REAL)],
            return_type: Some(TypeId::DINT),
        });
    }

    if let Some(dst_name) = upper.strip_prefix("TRUNC_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY_REAL)],
            return_type: Some(dst),
        });
    }

    if let Some((_, dst_name)) = upper.split_once("_TRUNC_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY_REAL)],
            return_type: Some(dst),
        });
    }

    if let Some(dst_name) = upper.strip_prefix("TO_BCD_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY_UNSIGNED)],
            return_type: Some(dst),
        });
    }

    if let Some((dst_name, _)) = upper.split_once("_TO_BCD_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY_UNSIGNED)],
            return_type: Some(dst),
        });
    }

    if let Some(dst_name) = upper.strip_prefix("BCD_TO_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY_BIT)],
            return_type: Some(dst),
        });
    }

    if let Some((_, dst_name)) = upper.split_once("_BCD_TO_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY_BIT)],
            return_type: Some(dst),
        });
    }

    if let Some(dst_name) = upper.strip_prefix("TO_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY)],
            return_type: Some(dst),
        });
    }

    if let Some((_, dst_name)) = upper.split_once("_TO_") {
        let dst = TypeId::from_builtin_name(dst_name)?;
        return Some(SignatureInfo {
            name: SmolStr::new(name),
            params: vec![param("IN", TypeId::ANY)],
            return_type: Some(dst),
        });
    }

    None
}

fn param(name: &str, type_id: TypeId) -> ParamData {
    ParamData {
        name: SmolStr::new(name),
        type_id,
        direction: ParamDirection::In,
    }
}

fn out_param(name: &str, type_id: TypeId) -> ParamData {
    ParamData {
        name: SmolStr::new(name),
        type_id,
        direction: ParamDirection::Out,
    }
}

fn fixed_in(prefix: &str, count: usize, type_id: TypeId) -> Vec<ParamData> {
    (1..=count)
        .map(|index| param(&format!("{}{}", prefix, index), type_id))
        .collect()
}

fn variadic_in(prefix: &str, count: usize, min: usize, type_id: TypeId) -> Vec<ParamData> {
    let total = std::cmp::max(count, min);
    fixed_in(prefix, total, type_id)
}

fn mux_params(arg_count: usize) -> Vec<ParamData> {
    let mut params = vec![param("K", TypeId::ANY_INT)];
    let inputs = std::cmp::max(arg_count.saturating_sub(1), 2);
    for index in 0..inputs {
        params.push(param(&format!("IN{}", index), TypeId::ANY));
    }
    params
}

fn time_binary(lhs: TypeId, rhs: TypeId) -> Vec<ParamData> {
    vec![param("IN1", lhs), param("IN2", rhs)]
}
