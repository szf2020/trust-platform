use super::super::*;
use super::*;
use crate::symbols::VarQualifier;

impl<'a, 'b> CallChecker<'a, 'b> {
    pub(in crate::type_check) fn infer_ref_call(&mut self, node: &SyntaxNode) -> TypeId {
        let args = self.collect_call_args(node);
        if args.len() != 1 {
            self.checker.diagnostics.error(
                DiagnosticCode::WrongArgumentCount,
                node.text_range(),
                format!("expected 1 argument, found {}", args.len()),
            );
            return TypeId::UNKNOWN;
        }
        let expr = &args[0].expr;
        if !self.checker.is_valid_lvalue(expr) {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                expr.text_range(),
                "REF expects an assignable operand",
            );
            return TypeId::UNKNOWN;
        }
        if self.checker.is_constant_target(expr) {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                expr.text_range(),
                "REF cannot take a reference to a constant",
            );
            return TypeId::UNKNOWN;
        }

        if let Some(resolved) = self.checker.resolve().resolve_lvalue_root(expr) {
            if let Some(symbol) = self.checker.symbols.get(resolved.id) {
                if matches!(
                    symbol.kind,
                    SymbolKind::Variable {
                        qualifier: VarQualifier::Temp
                    }
                ) {
                    self.checker.diagnostics.error(
                        DiagnosticCode::InvalidOperation,
                        expr.text_range(),
                        "REF cannot take a reference to a temporary variable",
                    );
                    return TypeId::UNKNOWN;
                }

                if let Some(current_id) = self.checker.current_pou_symbol {
                    if let Some(owner) = self.checker.symbols.get(current_id) {
                        let is_function_like = matches!(
                            owner.kind,
                            SymbolKind::Function { .. } | SymbolKind::Method { .. }
                        );
                        if is_function_like && symbol.parent == Some(current_id) {
                            self.checker.diagnostics.error(
                                DiagnosticCode::InvalidOperation,
                                expr.text_range(),
                                "REF cannot take a reference to function-local variables",
                            );
                            return TypeId::UNKNOWN;
                        }
                    }
                }
            }
        }

        let target_type = self.checker.expr().check_expression(expr);
        if target_type == TypeId::UNKNOWN {
            return TypeId::UNKNOWN;
        }

        self.checker.symbols.register_reference_type(target_type)
    }

    pub(in crate::type_check) fn infer_new_call(&mut self, node: &SyntaxNode) -> TypeId {
        let args = self.collect_call_args(node);
        if args.len() != 1 {
            self.checker.diagnostics.error(
                DiagnosticCode::WrongArgumentCount,
                node.text_range(),
                format!("expected 1 argument, found {}", args.len()),
            );
            return TypeId::UNKNOWN;
        }

        let arg = &args[0];
        if arg.assign != CallArgAssign::Positional {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidArgumentType,
                arg.range,
                "NEW expects a single positional type argument",
            );
            return TypeId::UNKNOWN;
        }

        let Some(target_type) = self.checker.resolve_ref().resolve_type_from_expr(&arg.expr) else {
            self.checker.diagnostics.error(
                DiagnosticCode::UndefinedType,
                arg.range,
                "NEW expects a type name",
            );
            return TypeId::UNKNOWN;
        };

        self.checker.symbols.register_reference_type(target_type)
    }

    pub(in crate::type_check) fn infer_ref_delete_call(&mut self, node: &SyntaxNode) -> TypeId {
        let args = self.collect_call_args(node);
        if args.len() != 1 {
            self.checker.diagnostics.error(
                DiagnosticCode::WrongArgumentCount,
                node.text_range(),
                format!("expected 1 argument, found {}", args.len()),
            );
            return TypeId::UNKNOWN;
        }

        let arg = &args[0];
        if arg.assign != CallArgAssign::Positional {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidArgumentType,
                arg.range,
                "__DELETE expects a single positional argument",
            );
            return TypeId::UNKNOWN;
        }

        let arg_type = self.checker.expr().check_expression(&arg.expr);
        let resolved = self.checker.resolve_alias_type(arg_type);
        match self.checker.symbols.type_by_id(resolved) {
            Some(Type::Reference { .. } | Type::Pointer { .. }) | Some(Type::Null) => {}
            _ => {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "__DELETE expects a REF_TO, POINTER TO, or NULL argument",
                );
                return TypeId::UNKNOWN;
            }
        }

        TypeId::VOID
    }
}
