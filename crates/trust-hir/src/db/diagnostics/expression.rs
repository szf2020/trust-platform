use super::super::*;

pub(in crate::db) fn expression_by_id(root: &SyntaxNode, expr_id: u32) -> Option<SyntaxNode> {
    for (index, node) in root
        .descendants()
        .filter(|node| is_expression_kind(node.kind()))
        .enumerate()
    {
        let Ok(index) = u32::try_from(index) else {
            break;
        };
        if index == expr_id {
            return Some(node);
        }
    }
    None
}

pub(in crate::db) fn expression_id_at_offset(root: &SyntaxNode, offset: TextSize) -> Option<u32> {
    let target = find_expression_at_offset(root, offset)?;
    expression_id_for_node(root, &target)
}

pub(in crate::db) fn expression_id_for_node(root: &SyntaxNode, target: &SyntaxNode) -> Option<u32> {
    for (index, node) in root
        .descendants()
        .filter(|node| is_expression_kind(node.kind()))
        .enumerate()
    {
        if &node == target {
            return u32::try_from(index).ok();
        }
    }
    None
}

pub(in crate::db) fn find_expression_at_offset(
    root: &SyntaxNode,
    offset: TextSize,
) -> Option<SyntaxNode> {
    root.descendants()
        .filter(|node| is_expression_kind(node.kind()))
        .filter(|node| node.text_range().contains(offset))
        .min_by_key(|node| node.text_range().len())
}

pub(in crate::db) fn is_expression_kind(kind: SyntaxKind) -> bool {
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
