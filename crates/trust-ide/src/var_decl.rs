use text_size::TextRange;

use trust_syntax::syntax::{SyntaxKind, SyntaxNode};

use crate::util::ident_token_in_name;

#[derive(Default, Clone)]
pub(crate) struct VarDeclInfo {
    pub(crate) initializer: Option<String>,
    pub(crate) retention: Option<&'static str>,
}

pub(crate) fn var_decl_info_for_symbol(
    root: &SyntaxNode,
    source: &str,
    symbol_range: TextRange,
) -> VarDeclInfo {
    let Some(var_decl) = find_var_decl_for_range(root, symbol_range) else {
        return VarDeclInfo::default();
    };
    let initializer = initializer_from_var_decl(source, &var_decl);
    let retention = var_decl
        .ancestors()
        .find(|node| node.kind() == SyntaxKind::VarBlock)
        .and_then(|node| retention_from_var_block(&node));
    VarDeclInfo {
        initializer,
        retention,
    }
}

pub(crate) fn find_var_decl_for_range(
    root: &SyntaxNode,
    symbol_range: TextRange,
) -> Option<SyntaxNode> {
    root.descendants()
        .filter(|node| node.kind() == SyntaxKind::VarDecl)
        .find(|var_decl| {
            var_decl
                .children()
                .filter(|node| node.kind() == SyntaxKind::Name)
                .filter_map(|node| ident_token_in_name(&node))
                .any(|ident| ident.text_range() == symbol_range)
        })
}

pub(crate) fn initializer_from_var_decl(source: &str, var_decl: &SyntaxNode) -> Option<String> {
    let expr = var_decl
        .children()
        .find(|node| is_expression_kind(node.kind()))?;
    let text = text_for_range(source, expr.text_range());
    (!text.is_empty()).then_some(text)
}

fn retention_from_var_block(block: &SyntaxNode) -> Option<&'static str> {
    for token in block
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
    {
        match token.kind() {
            SyntaxKind::KwRetain => return Some("RETAIN"),
            SyntaxKind::KwNonRetain => return Some("NON_RETAIN"),
            SyntaxKind::KwPersistent => return Some("PERSISTENT"),
            _ => {}
        }
    }
    None
}

fn text_for_range(source: &str, range: TextRange) -> String {
    let start: usize = range.start().into();
    let end: usize = range.end().into();
    source
        .get(start..end)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Literal
            | SyntaxKind::NameRef
            | SyntaxKind::BinaryExpr
            | SyntaxKind::UnaryExpr
            | SyntaxKind::CallExpr
            | SyntaxKind::IndexExpr
            | SyntaxKind::FieldExpr
            | SyntaxKind::DerefExpr
            | SyntaxKind::AddrExpr
            | SyntaxKind::ParenExpr
            | SyntaxKind::ThisExpr
            | SyntaxKind::SuperExpr
            | SyntaxKind::SizeOfExpr
    )
}
