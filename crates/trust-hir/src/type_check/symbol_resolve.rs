use super::calls::ResolvedSymbol;
use super::*;

impl<'a, 'b> ResolveChecker<'a, 'b> {
    pub(super) fn resolve_lvalue_root(&mut self, node: &SyntaxNode) -> Option<ResolvedSymbol> {
        let root = self.checker.resolve_ref().lvalue_root_name_ref(node)?;
        let name = self.checker.resolve_ref().get_name_from_ref(&root)?;
        self.resolve_name_in_context(&name, root.text_range())
    }
}

impl<'a, 'b> ResolveCheckerRef<'a, 'b> {
    pub(super) fn get_name_from_ref(&self, node: &SyntaxNode) -> Option<SmolStr> {
        for token in node
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
        {
            if matches!(
                token.kind(),
                SyntaxKind::Ident
                    | SyntaxKind::KwEn
                    | SyntaxKind::KwEno
                    | SyntaxKind::KwRef
                    | SyntaxKind::KwNew
                    | SyntaxKind::KwNewDunder
                    | SyntaxKind::KwDeleteDunder
            ) {
                return Some(SmolStr::new(token.text()));
            }
        }
        None
    }

    pub(super) fn lvalue_root_name_ref(&self, node: &SyntaxNode) -> Option<SyntaxNode> {
        match node.kind() {
            SyntaxKind::NameRef => Some(node.clone()),
            SyntaxKind::ParenExpr => node
                .children()
                .next()
                .and_then(|inner| self.lvalue_root_name_ref(&inner)),
            SyntaxKind::FieldExpr | SyntaxKind::IndexExpr => node
                .children()
                .next()
                .and_then(|base| self.lvalue_root_name_ref(&base)),
            SyntaxKind::DerefExpr => None,
            _ => None,
        }
    }

    pub(in crate::type_check) fn resolve_simple_symbol(
        &self,
        node: &SyntaxNode,
    ) -> Option<SymbolId> {
        if node.kind() == SyntaxKind::ParenExpr {
            if let Some(inner) = node.children().next() {
                return self.resolve_simple_symbol(&inner);
            }
        }

        if node.kind() != SyntaxKind::NameRef {
            return None;
        }

        let name = self.get_name_from_ref(node)?;
        self.checker
            .symbols
            .resolve(&name, self.checker.current_scope)
    }

    pub(super) fn resolve_type_from_expr(&self, node: &SyntaxNode) -> Option<TypeId> {
        match node.kind() {
            SyntaxKind::NameRef => {
                let name = self.get_name_from_ref(node)?;
                self.resolve_type_by_name(name.as_str())
            }
            SyntaxKind::FieldExpr => {
                let parts = self.qualified_name_from_field_expr(node)?;
                let symbol_id = self.checker.symbols.resolve_qualified(&parts)?;
                self.checker
                    .symbols
                    .get(symbol_id)
                    .and_then(|sym| sym.is_type().then_some(sym.type_id))
            }
            SyntaxKind::ParenExpr => node
                .children()
                .next()
                .and_then(|child| self.resolve_type_from_expr(&child)),
            _ => None,
        }
    }

    pub(super) fn resolve_type_by_name(&self, name: &str) -> Option<TypeId> {
        if let Some(id) = TypeId::from_builtin_name(name) {
            return Some(id);
        }
        if name.contains('.') {
            let parts: Vec<SmolStr> = name.split('.').map(SmolStr::new).collect();
            let symbol_id = self.checker.symbols.resolve_qualified(&parts)?;
            return self
                .checker
                .symbols
                .get(symbol_id)
                .and_then(|sym| sym.is_type().then_some(sym.type_id));
        }

        if let Some(symbol_id) = self
            .checker
            .symbols
            .resolve(name, self.checker.current_scope)
        {
            if let Some(symbol) = self.checker.symbols.get(symbol_id) {
                if symbol.is_type() {
                    return Some(symbol.type_id);
                }
            }
        }

        self.checker.symbols.lookup_type(name)
    }
}
