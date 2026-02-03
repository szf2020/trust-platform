use super::*;

impl<'a, 'b> StmtChecker<'a, 'b> {
    // ========== Statement Checking ==========

    /// Checks a statement for type errors.
    pub fn check_statement(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::AssignStmt => self.check_assignment(node),
            SyntaxKind::IfStmt => self.check_if_stmt(node),
            SyntaxKind::ForStmt => self.check_for_stmt(node),
            SyntaxKind::WhileStmt => self.check_while_stmt(node),
            SyntaxKind::RepeatStmt => self.check_repeat_stmt(node),
            SyntaxKind::CaseStmt => self.check_case_stmt(node),
            SyntaxKind::ReturnStmt => self.check_return_stmt(node),
            SyntaxKind::ExprStmt => self.check_expr_stmt(node),
            SyntaxKind::ExitStmt => self.check_exit_stmt(node),
            SyntaxKind::ContinueStmt => self.check_continue_stmt(node),
            SyntaxKind::JmpStmt => self.check_jmp_stmt(node),
            SyntaxKind::LabelStmt => self.check_label_stmt(node),
            SyntaxKind::StmtList => {
                for child in node.children() {
                    self.check_statement(&child);
                }
            }
            _ => {}
        }
    }

    fn check_expression(&mut self, node: &SyntaxNode) -> TypeId {
        self.checker.expr().check_expression(node)
    }

    fn check_assignment(&mut self, node: &SyntaxNode) {
        let children: Vec<_> = node.children().collect();
        if children.len() < 2 {
            return;
        }

        let target = &children[0];
        let value = &children[1];
        let is_ref_assign = assignment_is_ref(node);

        // Check target is a valid l-value
        if !self.checker.is_valid_lvalue(target) {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidAssignmentTarget,
                target.text_range(),
                "invalid assignment target",
            );
            return;
        }

        let resolved_target = self.checker.assignment_target_symbol(target);
        if let Some(resolved) = &resolved_target {
            if !resolved.accessible {
                return;
            }
        }

        // Check target is not a constant
        if self
            .checker
            .is_constant_target_with_resolved(target, resolved_target.as_ref())
        {
            self.checker.diagnostics.error(
                DiagnosticCode::ConstantModification,
                target.text_range(),
                "cannot assign to constant",
            );
            return;
        }

        if !self
            .checker
            .check_assignable_target_symbol(target, resolved_target.as_ref())
        {
            return;
        }

        if let Some(resolved) = &resolved_target {
            self.check_loop_restriction(resolved.id, target.text_range());
        }

        if self.checker.is_return_target(target) {
            self.checker.saw_return_value = true;
        }

        if is_ref_assign {
            self.check_ref_assignment(target, value);
            return;
        }

        // Check type compatibility
        let target_type = self
            .checker
            .type_of_assignment_target(target, resolved_target.as_ref());
        let value_type = self.check_expression(value);

        let is_context_int = self.checker.is_contextual_int_literal(target_type, value);
        let is_context_real = self.checker.is_contextual_real_literal(target_type, value);
        if self.checker.is_assignable(target_type, value_type) || is_context_int || is_context_real
        {
            let checked_type = if is_context_int || is_context_real {
                target_type
            } else {
                value_type
            };
            self.check_subrange_assignment(target_type, value, checked_type);
            self.checker
                .check_string_literal_assignment(target_type, value, checked_type);
            if !is_context_int && !is_context_real {
                self.checker
                    .warn_implicit_conversion(target_type, value_type, node.text_range());
            }
        } else {
            let target_name = self.checker.type_name(target_type);
            let value_name = self.checker.type_name(value_type);
            self.checker.diagnostics.error(
                DiagnosticCode::IncompatibleAssignment,
                node.text_range(),
                format!("cannot assign '{}' to '{}'", value_name, target_name),
            );
        }
    }

    fn check_if_stmt(&mut self, node: &SyntaxNode) {
        // Check condition is boolean
        if let Some(expr) = first_expression_child(node) {
            let cond_type = self.check_expression(&expr);
            self.checker
                .expr()
                .check_boolean(cond_type, expr.text_range());
        }

        // Check nested statements
        for child in node.children() {
            match child.kind() {
                SyntaxKind::ElsifBranch | SyntaxKind::ElseBranch => {
                    if child.kind() == SyntaxKind::ElsifBranch {
                        if let Some(expr) = first_expression_child(&child) {
                            let cond_type = self.check_expression(&expr);
                            self.checker
                                .expr()
                                .check_boolean(cond_type, expr.text_range());
                        }
                    }
                    self.check_statement_children(&child);
                }
                _ if is_statement_kind(child.kind()) => self.check_statement(&child),
                _ => {}
            }
        }
    }

    fn check_for_stmt(&mut self, node: &SyntaxNode) {
        let mut control_symbol = None;
        let mut control_type = None;

        // FOR loop iterator must be integer
        for child in node.children() {
            if matches!(child.kind(), SyntaxKind::NameRef | SyntaxKind::Name) {
                if let Some(name) = self.checker.resolve_ref().get_name_from_ref(&child) {
                    if let Some(symbol_id) = self
                        .checker
                        .symbols
                        .resolve(&name, self.checker.current_scope)
                    {
                        if let Some(symbol) = self.checker.symbols.get(symbol_id) {
                            let iter_type = self.checker.resolve_alias_type(symbol.type_id);
                            if let Some(ty) = self.checker.symbols.type_by_id(iter_type) {
                                if !ty.is_integer() {
                                    self.checker.diagnostics.error(
                                        DiagnosticCode::TypeMismatch,
                                        child.text_range(),
                                        "FOR loop iterator must be integer type",
                                    );
                                }
                            }
                            control_symbol = Some(symbol_id);
                            control_type = Some(iter_type);
                        }
                    }
                }
                break;
            }
        }

        let exprs: Vec<_> = node
            .children()
            .filter(|child| is_expression_kind(child.kind()))
            .collect();

        for (idx, expr) in exprs.iter().enumerate() {
            let expr_type_raw = self.check_expression(expr);
            let expr_type = self.checker.resolve_alias_type(expr_type_raw);
            if let Some(ty) = self.checker.symbols.type_by_id(expr_type) {
                if !ty.is_integer() {
                    self.checker.diagnostics.error(
                        DiagnosticCode::TypeMismatch,
                        expr.text_range(),
                        "FOR loop bounds must be integer type",
                    );
                }
            }

            if let Some(control_type) = control_type {
                let context_literal = self.checker.is_contextual_int_literal(control_type, expr);
                if expr_type != TypeId::UNKNOWN && expr_type != control_type && !context_literal {
                    let label = match idx {
                        0 => "initial value",
                        1 => "final value",
                        _ => "step value",
                    };
                    self.checker.diagnostics.error(
                        DiagnosticCode::TypeMismatch,
                        expr.text_range(),
                        format!(
                            "FOR loop {} must match control variable type '{}'",
                            label,
                            self.checker.type_name(control_type)
                        ),
                    );
                }
            }
        }

        let mut restricted = FxHashSet::default();
        if let Some(control_symbol) = control_symbol {
            restricted.insert(control_symbol);
        }
        if let Some(expr) = exprs.first() {
            if let Some(symbol_id) = self.checker.resolve_ref().resolve_simple_symbol(expr) {
                restricted.insert(symbol_id);
            }
        }
        if let Some(expr) = exprs.get(1) {
            if let Some(symbol_id) = self.checker.resolve_ref().resolve_simple_symbol(expr) {
                restricted.insert(symbol_id);
            }
        }

        self.checker.loop_stack.push(LoopContext { restricted });
        self.check_statement_children(node);
        self.checker.loop_stack.pop();
    }

    fn check_while_stmt(&mut self, node: &SyntaxNode) {
        // Check condition is boolean
        if let Some(expr) = first_expression_child(node) {
            let cond_type = self.check_expression(&expr);
            self.checker
                .expr()
                .check_boolean(cond_type, expr.text_range());
        }

        self.checker.loop_stack.push(LoopContext {
            restricted: FxHashSet::default(),
        });
        self.check_statement_children(node);
        self.checker.loop_stack.pop();
    }

    fn check_repeat_stmt(&mut self, node: &SyntaxNode) {
        // Check UNTIL condition is boolean
        if let Some(expr) = last_expression_child(node) {
            let cond_type = self.check_expression(&expr);
            self.checker
                .expr()
                .check_boolean(cond_type, expr.text_range());
        }

        self.checker.loop_stack.push(LoopContext {
            restricted: FxHashSet::default(),
        });
        self.check_statement_children(node);
        self.checker.loop_stack.pop();
    }

    fn check_case_stmt(&mut self, node: &SyntaxNode) {
        // Get selector type
        let mut selector_type = TypeId::UNKNOWN;
        if let Some(expr) = first_expression_child(node) {
            selector_type = self.check_expression(&expr);
            if selector_type != TypeId::UNKNOWN && !self.is_case_selector_type(selector_type) {
                self.checker.diagnostics.error(
                    DiagnosticCode::TypeMismatch,
                    expr.text_range(),
                    "CASE selector must be an elementary type",
                );
            }
        }

        let mut tracker = CaseLabelTracker::default();

        // Check case branches
        for child in node.children() {
            if child.kind() == SyntaxKind::CaseBranch {
                self.check_case_branch(&child, selector_type, &mut tracker);
            }
        }

        let has_else = node
            .children()
            .any(|child| child.kind() == SyntaxKind::ElseBranch);
        if !has_else && !self.case_labels_cover_enum(selector_type, &tracker) {
            self.checker.diagnostics.warning(
                DiagnosticCode::MissingElse,
                node.text_range(),
                "CASE statement has no ELSE branch",
            );
        }
    }

    fn check_exit_stmt(&mut self, node: &SyntaxNode) {
        if self.checker.loop_stack.is_empty() {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                node.text_range(),
                "EXIT must appear inside a loop",
            );
        }
    }

    fn check_continue_stmt(&mut self, node: &SyntaxNode) {
        if self.checker.loop_stack.is_empty() {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                node.text_range(),
                "CONTINUE must appear inside a loop",
            );
        }
    }

    fn check_jmp_stmt(&mut self, node: &SyntaxNode) {
        let label_name = node
            .children()
            .find(|n| n.kind() == SyntaxKind::Name)
            .and_then(|name| self.checker.resolve_ref().get_name_from_ref(&name));

        let Some(label_name) = label_name else {
            return;
        };

        if let Some(scope) = self.checker.label_scopes.last_mut() {
            let key = SmolStr::new(label_name.to_ascii_uppercase());
            if scope.labels.contains(&key) {
                return;
            }
            scope
                .pending_jumps
                .push((key, label_name.clone(), node.text_range()));
        } else {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                node.text_range(),
                "JMP is not valid outside a statement list",
            );
        }
    }

    fn check_label_stmt(&mut self, node: &SyntaxNode) {
        if let Some(label_node) = node.children().find(|n| n.kind() == SyntaxKind::Name) {
            if let Some(name) = self.checker.resolve_ref().get_name_from_ref(&label_node) {
                if let Some(scope) = self.checker.label_scopes.last_mut() {
                    let key = SmolStr::new(name.to_ascii_uppercase());
                    if !scope.labels.insert(key) {
                        self.checker.diagnostics.error(
                            DiagnosticCode::DuplicateDeclaration,
                            label_node.text_range(),
                            format!("duplicate label '{}'", name),
                        );
                    }
                }
            }
        }

        self.check_statement_children(node);
    }

    fn check_case_branch(
        &mut self,
        node: &SyntaxNode,
        selector_type: TypeId,
        tracker: &mut CaseLabelTracker,
    ) {
        // Check that case labels are compatible with selector type
        for child in node.children() {
            match child.kind() {
                SyntaxKind::CaseLabel => self.check_case_label(&child, selector_type, tracker),
                SyntaxKind::Subrange => self.check_case_subrange(&child, selector_type, tracker),
                _ if is_expression_kind(child.kind()) => {
                    self.check_case_label_expr(&child, selector_type, tracker);
                }
                _ if is_statement_kind(child.kind()) => self.check_statement(&child),
                _ => {}
            }
        }
    }

    fn check_case_label(
        &mut self,
        node: &SyntaxNode,
        selector_type: TypeId,
        tracker: &mut CaseLabelTracker,
    ) {
        if let Some(subrange) = node.children().find(|n| n.kind() == SyntaxKind::Subrange) {
            self.check_case_subrange(&subrange, selector_type, tracker);
            return;
        }

        if let Some(expr) = node.children().find(|n| is_expression_kind(n.kind())) {
            self.check_case_label_expr(&expr, selector_type, tracker);
        }
    }

    fn check_case_subrange(
        &mut self,
        node: &SyntaxNode,
        selector_type: TypeId,
        tracker: &mut CaseLabelTracker,
    ) {
        let mut bounds = Vec::new();
        let mut has_label = false;
        for child in node.children().filter(|n| is_expression_kind(n.kind())) {
            has_label = true;
            if !self.is_case_label_expr(&child) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidOperation,
                    child.text_range(),
                    "case label must be a literal, enum value, or constant",
                );
                continue;
            }
            let label_type = self.check_expression(&child);
            if !self.checker.is_assignable(selector_type, label_type)
                && !self
                    .checker
                    .is_contextual_int_literal(selector_type, &child)
                && !self
                    .checker
                    .is_contextual_real_literal(selector_type, &child)
            {
                self.checker.diagnostics.error(
                    DiagnosticCode::TypeMismatch,
                    child.text_range(),
                    "case label type must match selector type",
                );
            }
            if let Some(value) = self.checker.eval_const_int_expr(&child) {
                bounds.push(value);
            }
        }

        match bounds.len() {
            1 => self.record_case_label_value(tracker, bounds[0], node.text_range()),
            2 => self.record_case_label_range(tracker, bounds[0], bounds[1], node.text_range()),
            _ => {}
        }

        if !has_label && node.kind() == SyntaxKind::Subrange {
            self.checker.diagnostics.error(
                DiagnosticCode::TypeMismatch,
                node.text_range(),
                "case label type must match selector type",
            );
        }
    }

    fn check_case_label_expr(
        &mut self,
        expr: &SyntaxNode,
        selector_type: TypeId,
        tracker: &mut CaseLabelTracker,
    ) {
        if !self.is_case_label_expr(expr) {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                expr.text_range(),
                "case label must be a literal, enum value, or constant",
            );
            return;
        }

        let label_type = self.check_expression(expr);
        if !self.checker.is_assignable(selector_type, label_type)
            && !self.checker.is_contextual_int_literal(selector_type, expr)
            && !self.checker.is_contextual_real_literal(selector_type, expr)
        {
            self.checker.diagnostics.error(
                DiagnosticCode::TypeMismatch,
                expr.text_range(),
                "case label type must match selector type",
            );
        }

        if let Some(value) = self.checker.eval_const_int_expr(expr) {
            self.record_case_label_value(tracker, value, expr.text_range());
        }
    }

    fn is_case_label_expr(&mut self, expr: &SyntaxNode) -> bool {
        match expr.kind() {
            SyntaxKind::Literal => true,
            SyntaxKind::ParenExpr => expr
                .children()
                .find(|child| is_expression_kind(child.kind()))
                .is_some_and(|child| self.is_case_label_expr(&child)),
            SyntaxKind::UnaryExpr => {
                let is_neg = expr
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|token| token.kind() == SyntaxKind::Minus);
                if !is_neg {
                    return false;
                }
                expr.children()
                    .find(|child| is_expression_kind(child.kind()))
                    .is_some_and(|child| self.is_case_label_expr(&child))
            }
            SyntaxKind::NameRef => {
                let Some(name) = self.checker.resolve_ref().get_name_from_ref(expr) else {
                    return false;
                };
                let Some(symbol_id) = self
                    .checker
                    .symbols
                    .resolve(&name, self.checker.current_scope)
                else {
                    return false;
                };
                let Some(symbol) = self.checker.symbols.get(symbol_id) else {
                    return false;
                };
                matches!(
                    symbol.kind,
                    SymbolKind::Constant | SymbolKind::EnumValue { .. }
                )
            }
            _ => false,
        }
    }

    fn is_case_selector_type(&self, type_id: TypeId) -> bool {
        let resolved = self.checker.resolve_alias_type(type_id);
        matches!(
            self.checker.symbols.type_by_id(resolved),
            Some(
                Type::Bool
                    | Type::SInt
                    | Type::Int
                    | Type::DInt
                    | Type::LInt
                    | Type::USInt
                    | Type::UInt
                    | Type::UDInt
                    | Type::ULInt
                    | Type::Real
                    | Type::LReal
                    | Type::Byte
                    | Type::Word
                    | Type::DWord
                    | Type::LWord
                    | Type::Time
                    | Type::LTime
                    | Type::Date
                    | Type::LDate
                    | Type::Tod
                    | Type::LTod
                    | Type::Dt
                    | Type::Ldt
                    | Type::String { .. }
                    | Type::WString { .. }
                    | Type::Char
                    | Type::WChar
                    | Type::Enum { .. }
                    | Type::Subrange { .. }
                    | Type::Any
                    | Type::AnyInt
                    | Type::AnyReal
                    | Type::AnyNum
                    | Type::AnyBit
                    | Type::AnyString
                    | Type::AnyDate
            )
        )
    }

    fn case_labels_cover_enum(&self, selector_type: TypeId, tracker: &CaseLabelTracker) -> bool {
        let resolved = self.checker.resolve_alias_type(selector_type);
        let Some(Type::Enum { values, .. }) = self.checker.symbols.type_by_id(resolved) else {
            return false;
        };
        if values.is_empty() {
            return false;
        }
        values.iter().all(|(_, value)| tracker.covers(*value))
    }

    fn check_return_stmt(&mut self, node: &SyntaxNode) {
        let return_expr = node.children().find(|n| is_expression_kind(n.kind()));
        if return_expr.is_some() {
            self.checker.saw_return_value = true;
        }

        match (self.checker.current_function_return, return_expr) {
            (Some(expected), Some(expr)) if expected != TypeId::VOID => {
                let actual = self.check_expression(&expr);
                if !self.checker.is_assignable(expected, actual)
                    && !self.checker.is_contextual_int_literal(expected, &expr)
                    && !self.checker.is_contextual_real_literal(expected, &expr)
                {
                    self.checker.diagnostics.error(
                        DiagnosticCode::InvalidReturnType,
                        expr.text_range(),
                        format!(
                            "return type mismatch: expected '{}', found '{}'",
                            self.checker.type_name(expected),
                            self.checker.type_name(actual)
                        ),
                    );
                }
            }
            (Some(expected), None) if expected != TypeId::VOID => {
                self.checker.diagnostics.error(
                    DiagnosticCode::MissingReturn,
                    node.text_range(),
                    "missing return value",
                );
            }
            (None, Some(expr)) | (Some(TypeId::VOID), Some(expr)) => {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidReturnType,
                    expr.text_range(),
                    "unexpected return value in procedure",
                );
            }
            _ => {}
        }
    }

    /// Emits missing return diagnostics after statement checks.
    pub fn finish_return_checks(&mut self, node: &SyntaxNode) {
        if let Some(expected) = self.checker.current_function_return {
            if expected != TypeId::VOID && !self.checker.saw_return_value {
                self.checker.diagnostics.error(
                    DiagnosticCode::MissingReturn,
                    node.text_range(),
                    "missing return value",
                );
            }
        }
    }

    fn check_expr_stmt(&mut self, node: &SyntaxNode) {
        // Just type-check the expression to catch any errors
        if let Some(expr) = node.children().next() {
            self.check_expression(&expr);
        }
    }

    fn check_statement_children(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            if is_statement_kind(child.kind()) {
                self.check_statement(&child);
            }
        }
    }

    pub(super) fn check_loop_restriction(&mut self, symbol_id: SymbolId, range: TextRange) {
        for ctx in &self.checker.loop_stack {
            if ctx.restricted.contains(&symbol_id) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidOperation,
                    range,
                    "FOR loop control variables must not be modified in the loop body",
                );
                break;
            }
        }
    }

    fn record_case_label_value(
        &mut self,
        tracker: &mut CaseLabelTracker,
        value: i64,
        range: TextRange,
    ) {
        if tracker.ints.contains_key(&value)
            || tracker
                .ranges
                .iter()
                .any(|(lower, upper)| value >= *lower && value <= *upper)
        {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                range,
                "duplicate CASE label",
            );
            return;
        }

        tracker.ints.insert(value, range);
    }

    fn record_case_label_range(
        &mut self,
        tracker: &mut CaseLabelTracker,
        start: i64,
        end: i64,
        range: TextRange,
    ) {
        let (lower, upper) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let overlaps_value = tracker
            .ints
            .keys()
            .any(|value| *value >= lower && *value <= upper);
        let overlaps_range = tracker
            .ranges
            .iter()
            .any(|(r_lower, r_upper)| !(upper < *r_lower || lower > *r_upper));

        if overlaps_value || overlaps_range {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidOperation,
                range,
                "duplicate CASE label",
            );
            return;
        }

        tracker.ranges.push((lower, upper));
    }

    fn check_ref_assignment(&mut self, target: &SyntaxNode, value: &SyntaxNode) {
        let target_type = self.checker.expr().check_expression(target);
        let value_type = self.checker.expr().check_expression(value);

        if target_type == TypeId::UNKNOWN || value_type == TypeId::UNKNOWN {
            return;
        }

        let Some(target_ref) = self.reference_target_type(target_type) else {
            self.checker.diagnostics.error(
                DiagnosticCode::TypeMismatch,
                target.text_range(),
                "reference assignment requires REF_TO target",
            );
            return;
        };

        if value_type == TypeId::NULL {
            return;
        }

        let Some(source_ref) = self.reference_target_type(value_type) else {
            self.checker.diagnostics.error(
                DiagnosticCode::TypeMismatch,
                value.text_range(),
                "reference assignment requires REF_TO source",
            );
            return;
        };

        if !self
            .checker
            .reference_types_compatible(target_ref, source_ref)
        {
            let target_name = self.checker.type_name(target_ref);
            let source_name = self.checker.type_name(source_ref);
            self.checker.diagnostics.error(
                DiagnosticCode::TypeMismatch,
                value.text_range(),
                format!(
                    "reference assignment requires compatible types: '{}' vs '{}'",
                    target_name, source_name
                ),
            );
        }
    }

    fn reference_target_type(&self, type_id: TypeId) -> Option<TypeId> {
        let resolved = self.checker.resolve_alias_type(type_id);
        match self.checker.symbols.type_by_id(resolved)? {
            Type::Reference { target } => Some(*target),
            _ => None,
        }
    }

    fn check_subrange_assignment(
        &mut self,
        target_type: TypeId,
        value: &SyntaxNode,
        value_type: TypeId,
    ) {
        let Some((_, lower, upper)) = self.checker.subrange_bounds(target_type) else {
            return;
        };

        if let Some((_, value_lower, value_upper)) = self.checker.subrange_bounds(value_type) {
            if value_lower >= lower && value_upper <= upper {
                return;
            }
        }

        if let Some(value_int) = self.checker.eval_const_int_expr(value) {
            if value_int < lower || value_int > upper {
                self.checker.diagnostics.error(
                    DiagnosticCode::OutOfRange,
                    value.text_range(),
                    format!("value {} outside subrange {}..{}", value_int, lower, upper),
                );
            }
        }
    }

    pub(crate) fn check_statement_list_with_labels(&mut self, node: &SyntaxNode) {
        self.checker.label_scopes.push(LabelScope {
            labels: FxHashSet::default(),
            pending_jumps: Vec::new(),
        });
        self.check_statement(node);

        if let Some(scope) = self.checker.label_scopes.pop() {
            for (label, original, range) in scope.pending_jumps {
                if !scope.labels.contains(&label) {
                    self.checker.diagnostics.error(
                        DiagnosticCode::CannotResolve,
                        range,
                        format!("unknown label '{}'", original),
                    );
                }
            }
        }
    }
}

fn assignment_is_ref(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|e| e.into_token())
        .any(|token| token.kind() == SyntaxKind::RefAssign)
}
