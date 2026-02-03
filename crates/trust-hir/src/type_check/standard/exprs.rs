use super::super::*;

impl<'a> TypeChecker<'a> {
    pub(in crate::type_check) fn infer_paren_expr(&mut self, node: &SyntaxNode) -> TypeId {
        node.children()
            .next()
            .map(|child| self.expr().check_expression(&child))
            .unwrap_or(TypeId::UNKNOWN)
    }

    pub(in crate::type_check) fn infer_this_expr(&mut self, node: &SyntaxNode) -> TypeId {
        if let Some(ty) = self.this_type {
            return ty;
        }

        self.diagnostics.error(
            DiagnosticCode::CannotResolve,
            node.text_range(),
            "THIS is only valid inside function blocks or interfaces",
        );
        TypeId::UNKNOWN
    }

    pub(in crate::type_check) fn infer_super_expr(&mut self, node: &SyntaxNode) -> TypeId {
        if let Some(ty) = self.super_type {
            return ty;
        }

        self.diagnostics.error(
            DiagnosticCode::CannotResolve,
            node.text_range(),
            "SUPER is only valid when a base type is declared with EXTENDS",
        );
        TypeId::UNKNOWN
    }

    pub(in crate::type_check) fn infer_size_of_expr(&mut self, node: &SyntaxNode) -> TypeId {
        if let Some(expr) = node
            .children()
            .find(|child| is_expression_kind(child.kind()))
        {
            self.expr().check_expression(&expr);
        }

        TypeId::DINT
    }
}
