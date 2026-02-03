use smol_str::SmolStr;

use crate::bytecode::DebugEntry;
use crate::value::{Value, ValueRef};

use super::consts::type_id_for_value;
use super::util::to_u32;
use super::{AccessKind, BytecodeEncoder, BytecodeError, CodegenContext};

impl<'a> BytecodeEncoder<'a> {
    fn emit_assign(
        &mut self,
        ctx: &CodegenContext,
        target: &crate::eval::expr::LValue,
        value: &crate::eval::expr::Expr,
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        if let Some(emitted) = self.emit_dynamic_assign(ctx, target, value, code)? {
            return Ok(emitted);
        }
        let start_len = code.len();
        if !self.emit_expr(ctx, value, code)? {
            code.truncate(start_len);
            return Ok(false);
        }
        let reference = match self.resolve_lvalue_ref(ctx, target)? {
            Some(reference) => reference,
            None => {
                code.truncate(start_len);
                return Ok(false);
            }
        };
        let ref_idx = self.ref_index_for(&reference)?;
        code.push(0x21);
        code.extend_from_slice(&ref_idx.to_le_bytes());
        Ok(true)
    }

    fn emit_dynamic_assign(
        &mut self,
        ctx: &CodegenContext,
        target: &crate::eval::expr::LValue,
        value: &crate::eval::expr::Expr,
        code: &mut Vec<u8>,
    ) -> Result<Option<bool>, BytecodeError> {
        if !self.lvalue_is_self_field(ctx, target) {
            return Ok(None);
        }
        let start_len = code.len();
        if !self.emit_expr(ctx, value, code)? {
            code.truncate(start_len);
            return Ok(Some(false));
        }
        if !self.emit_dynamic_ref_for_lvalue(ctx, target, code)? {
            code.truncate(start_len);
            return Ok(Some(false));
        }
        code.push(0x13); // SWAP
        code.push(0x33); // STORE
        Ok(Some(true))
    }

    fn emit_dynamic_ref_for_lvalue(
        &mut self,
        ctx: &CodegenContext,
        target: &crate::eval::expr::LValue,
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        use crate::eval::expr::LValue;
        match target {
            LValue::Name(name) => self.emit_self_field_ref(ctx, name, code),
            LValue::Field { name, field } => {
                if !self.emit_self_field_ref(ctx, name, code)? {
                    return Ok(false);
                }
                let field_idx = self.strings.intern(field.clone());
                code.push(0x30);
                code.extend_from_slice(&field_idx.to_le_bytes());
                Ok(true)
            }
            LValue::Index { name, indices } => {
                if !self.emit_self_field_ref(ctx, name, code)? {
                    return Ok(false);
                }
                for index in indices {
                    if !self.emit_expr(ctx, index, code)? {
                        return Ok(false);
                    }
                    code.push(0x31);
                }
                Ok(true)
            }
            LValue::Deref(_) => Ok(false),
        }
    }

    fn lvalue_is_self_field(
        &self,
        ctx: &CodegenContext,
        target: &crate::eval::expr::LValue,
    ) -> bool {
        match target {
            crate::eval::expr::LValue::Name(name)
            | crate::eval::expr::LValue::Field { name, .. }
            | crate::eval::expr::LValue::Index { name, .. } => {
                ctx.self_field_name(name).is_some() && ctx.local_ref(name).is_none()
            }
            crate::eval::expr::LValue::Deref(_) => false,
        }
    }

    fn emit_self_field_ref(
        &mut self,
        ctx: &CodegenContext,
        name: &SmolStr,
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        let Some(field_name) = ctx.self_field_name(name) else {
            return Ok(false);
        };
        code.push(0x23);
        let name_idx = self.strings.intern(field_name.clone());
        code.push(0x30);
        code.extend_from_slice(&name_idx.to_le_bytes());
        Ok(true)
    }

    fn emit_load_access(
        &mut self,
        access: &AccessKind,
        code: &mut Vec<u8>,
    ) -> Result<(), BytecodeError> {
        match access {
            AccessKind::Static(reference) => self.emit_load_ref(reference, code),
            AccessKind::SelfField(field) => {
                code.push(0x23);
                let name_idx = self.strings.intern(field.clone());
                code.push(0x30);
                code.extend_from_slice(&name_idx.to_le_bytes());
                code.push(0x32);
                Ok(())
            }
        }
    }

    fn emit_store_access(
        &mut self,
        access: &AccessKind,
        code: &mut Vec<u8>,
    ) -> Result<(), BytecodeError> {
        match access {
            AccessKind::Static(reference) => self.emit_store_ref(reference, code),
            AccessKind::SelfField(field) => {
                code.push(0x23);
                let name_idx = self.strings.intern(field.clone());
                code.push(0x30);
                code.extend_from_slice(&name_idx.to_le_bytes());
                code.push(0x13);
                code.push(0x33);
                Ok(())
            }
        }
    }

    fn emit_dynamic_load_name(
        &mut self,
        ctx: &CodegenContext,
        name: &SmolStr,
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        if !self.emit_self_field_ref(ctx, name, code)? {
            return Ok(false);
        }
        code.push(0x32);
        Ok(true)
    }

    fn emit_dynamic_load_field(
        &mut self,
        ctx: &CodegenContext,
        base: &SmolStr,
        field: &SmolStr,
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        if !self.emit_self_field_ref(ctx, base, code)? {
            return Ok(false);
        }
        let field_idx = self.strings.intern(field.clone());
        code.push(0x30);
        code.extend_from_slice(&field_idx.to_le_bytes());
        code.push(0x32);
        Ok(true)
    }

    fn emit_dynamic_load_index(
        &mut self,
        ctx: &CodegenContext,
        base: &SmolStr,
        indices: &[crate::eval::expr::Expr],
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        if !self.emit_self_field_ref(ctx, base, code)? {
            return Ok(false);
        }
        for index in indices {
            if !self.emit_expr(ctx, index, code)? {
                return Ok(false);
            }
            code.push(0x31);
        }
        code.push(0x32);
        Ok(true)
    }

    fn emit_expr(
        &mut self,
        ctx: &CodegenContext,
        expr: &crate::eval::expr::Expr,
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        let start_len = code.len();
        let result = match expr {
            crate::eval::expr::Expr::Literal(value) => {
                let const_idx = match self.const_index_for(value) {
                    Ok(idx) => idx,
                    Err(_) => {
                        code.truncate(start_len);
                        return Ok(false);
                    }
                };
                code.push(0x10);
                code.extend_from_slice(&const_idx.to_le_bytes());
                Ok(true)
            }
            crate::eval::expr::Expr::Name(name) => {
                if let Some(reference) = ctx.local_ref(name) {
                    let ref_idx = self.ref_index_for(reference)?;
                    code.push(0x20);
                    code.extend_from_slice(&ref_idx.to_le_bytes());
                    return Ok(true);
                }
                if self.emit_dynamic_load_name(ctx, name, code)? {
                    return Ok(true);
                }
                let reference = match self.resolve_name_ref(ctx, name)? {
                    Some(reference) => reference,
                    None => {
                        code.truncate(start_len);
                        return Ok(false);
                    }
                };
                let ref_idx = self.ref_index_for(&reference)?;
                code.push(0x20);
                code.extend_from_slice(&ref_idx.to_le_bytes());
                Ok(true)
            }
            crate::eval::expr::Expr::Field { target, field } => {
                if let crate::eval::expr::Expr::Name(base) = target.as_ref() {
                    if self.emit_dynamic_load_field(ctx, base, field, code)? {
                        return Ok(true);
                    }
                    code.truncate(start_len);
                    let reference = match self.resolve_lvalue_ref(
                        ctx,
                        &crate::eval::expr::LValue::Field {
                            name: base.clone(),
                            field: field.clone(),
                        },
                    )? {
                        Some(reference) => reference,
                        None => {
                            code.truncate(start_len);
                            return Ok(false);
                        }
                    };
                    let ref_idx = self.ref_index_for(&reference)?;
                    code.push(0x20);
                    code.extend_from_slice(&ref_idx.to_le_bytes());
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            crate::eval::expr::Expr::Index { target, indices } => {
                if let crate::eval::expr::Expr::Name(base) = target.as_ref() {
                    if self.emit_dynamic_load_index(ctx, base, indices, code)? {
                        return Ok(true);
                    }
                    code.truncate(start_len);
                    let reference = match self.resolve_lvalue_ref(
                        ctx,
                        &crate::eval::expr::LValue::Index {
                            name: base.clone(),
                            indices: indices.clone(),
                        },
                    )? {
                        Some(reference) => reference,
                        None => {
                            code.truncate(start_len);
                            return Ok(false);
                        }
                    };
                    let ref_idx = self.ref_index_for(&reference)?;
                    code.push(0x20);
                    code.extend_from_slice(&ref_idx.to_le_bytes());
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            crate::eval::expr::Expr::Unary { op, expr } => {
                use crate::eval::ops::UnaryOp;
                if !self.emit_expr(ctx, expr, code)? {
                    code.truncate(start_len);
                    return Ok(false);
                }
                match op {
                    UnaryOp::Neg => code.push(0x45),
                    UnaryOp::Not => code.push(0x49),
                    UnaryOp::Pos => {}
                }
                Ok(true)
            }
            crate::eval::expr::Expr::Binary { op, left, right } => {
                use crate::eval::ops::BinaryOp;
                let opcode = match op {
                    BinaryOp::Add => 0x40,
                    BinaryOp::Sub => 0x41,
                    BinaryOp::Mul => 0x42,
                    BinaryOp::Div => 0x43,
                    BinaryOp::Mod => 0x44,
                    BinaryOp::Pow => 0x4C,
                    BinaryOp::And => 0x46,
                    BinaryOp::Or => 0x47,
                    BinaryOp::Xor => 0x48,
                    BinaryOp::Eq => 0x50,
                    BinaryOp::Ne => 0x51,
                    BinaryOp::Lt => 0x52,
                    BinaryOp::Le => 0x53,
                    BinaryOp::Gt => 0x54,
                    BinaryOp::Ge => 0x55,
                };
                if !self.emit_expr(ctx, left, code)? {
                    code.truncate(start_len);
                    return Ok(false);
                }
                if !self.emit_expr(ctx, right, code)? {
                    code.truncate(start_len);
                    return Ok(false);
                }
                code.push(opcode);
                Ok(true)
            }
            _ => Ok(false),
        };
        match result {
            Ok(true) => Ok(true),
            Ok(false) => {
                code.truncate(start_len);
                Ok(false)
            }
            Err(err) => {
                code.truncate(start_len);
                Err(err)
            }
        }
    }

    pub(super) fn emit_pou_body(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        body: &[crate::eval::stmt::Stmt],
    ) -> Result<(Vec<u8>, Vec<DebugEntry>), BytecodeError> {
        let mut code = Vec::new();
        let mut debug_entries = Vec::new();
        for stmt in body {
            self.emit_stmt(ctx, pou_id, stmt, &mut code, &mut debug_entries)?;
        }
        Ok((code, debug_entries))
    }

    fn emit_stmt(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        stmt: &crate::eval::stmt::Stmt,
        code: &mut Vec<u8>,
        debug_entries: &mut Vec<DebugEntry>,
    ) -> Result<(), BytecodeError> {
        let offset = to_u32(code.len(), "debug code offset")?;
        if let (Some(location), Some(sources)) = (stmt.location(), self.sources) {
            let source = sources
                .get(location.file_id as usize)
                .ok_or_else(|| BytecodeError::InvalidSection("debug source missing".into()))?;
            let (line, column) = crate::debug::location_to_line_col(source, location);
            let line = line.saturating_add(1);
            let column = column.saturating_add(1);
            let file_idx = self.file_path_index(location.file_id)?;
            debug_entries.push(DebugEntry {
                pou_id,
                code_offset: offset,
                file_idx,
                line,
                column,
                kind: 0,
            });
        }
        let emitted = match stmt {
            crate::eval::stmt::Stmt::Assign { target, value, .. } => {
                self.emit_assign(ctx, target, value, code)?
            }
            crate::eval::stmt::Stmt::If {
                condition,
                then_block,
                else_if,
                else_block,
                ..
            } => self.emit_if_stmt(
                ctx,
                pou_id,
                condition,
                then_block,
                else_if,
                else_block,
                code,
                debug_entries,
            )?,
            crate::eval::stmt::Stmt::Case {
                selector,
                branches,
                else_block,
                ..
            } => self.emit_case_stmt(
                ctx,
                pou_id,
                selector,
                branches,
                else_block,
                code,
                debug_entries,
            )?,
            crate::eval::stmt::Stmt::While {
                condition, body, ..
            } => self.emit_while_stmt(ctx, pou_id, condition, body, code, debug_entries)?,
            crate::eval::stmt::Stmt::Repeat { body, until, .. } => {
                self.emit_repeat_stmt(ctx, pou_id, body, until, code, debug_entries)?
            }
            crate::eval::stmt::Stmt::For {
                control,
                start,
                end,
                step,
                body,
                ..
            } => self.emit_for_stmt(
                ctx,
                pou_id,
                control,
                start,
                end,
                step,
                body,
                code,
                debug_entries,
            )?,
            crate::eval::stmt::Stmt::Label { stmt, .. } => {
                if let Some(stmt) = stmt.as_deref() {
                    self.emit_stmt(ctx, pou_id, stmt, code, debug_entries)?;
                    true
                } else {
                    false
                }
            }
            _ => false,
        };

        if !emitted {
            code.push(0x00);
        }
        Ok(())
    }

    fn emit_block(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        block: &[crate::eval::stmt::Stmt],
        code: &mut Vec<u8>,
        debug_entries: &mut Vec<DebugEntry>,
    ) -> Result<(), BytecodeError> {
        for stmt in block {
            self.emit_stmt(ctx, pou_id, stmt, code, debug_entries)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_if_stmt(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        condition: &crate::eval::expr::Expr,
        then_block: &[crate::eval::stmt::Stmt],
        else_if: &[(crate::eval::expr::Expr, Vec<crate::eval::stmt::Stmt>)],
        else_block: &[crate::eval::stmt::Stmt],
        code: &mut Vec<u8>,
        debug_entries: &mut Vec<DebugEntry>,
    ) -> Result<bool, BytecodeError> {
        let code_start = code.len();
        let debug_start = debug_entries.len();
        if !expr_supported(condition) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        for (cond, _) in else_if {
            if !expr_supported(cond) {
                code.truncate(code_start);
                debug_entries.truncate(debug_start);
                return Ok(false);
            }
        }
        if !self.emit_expr(ctx, condition, code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        let mut end_jumps = Vec::new();
        let mut jump_false = self.emit_jump_placeholder(code, 0x04);
        if let Err(err) = self.emit_block(ctx, pou_id, then_block, code, debug_entries) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Err(err);
        }
        if !else_if.is_empty() || !else_block.is_empty() {
            end_jumps.push(self.emit_jump_placeholder(code, 0x02));
        }
        let mut next_start = code.len();
        self.patch_jump(code, jump_false, next_start)?;
        for (cond, block) in else_if {
            if !self.emit_expr(ctx, cond, code)? {
                code.truncate(code_start);
                debug_entries.truncate(debug_start);
                return Ok(false);
            }
            jump_false = self.emit_jump_placeholder(code, 0x04);
            if let Err(err) = self.emit_block(ctx, pou_id, block, code, debug_entries) {
                code.truncate(code_start);
                debug_entries.truncate(debug_start);
                return Err(err);
            }
            end_jumps.push(self.emit_jump_placeholder(code, 0x02));
            next_start = code.len();
            self.patch_jump(code, jump_false, next_start)?;
        }
        if let Err(err) = self.emit_block(ctx, pou_id, else_block, code, debug_entries) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Err(err);
        }
        let end = code.len();
        for jump in end_jumps {
            self.patch_jump(code, jump, end)?;
        }
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_case_stmt(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        selector: &crate::eval::expr::Expr,
        branches: &[(
            Vec<crate::eval::stmt::CaseLabel>,
            Vec<crate::eval::stmt::Stmt>,
        )],
        else_block: &[crate::eval::stmt::Stmt],
        code: &mut Vec<u8>,
        debug_entries: &mut Vec<DebugEntry>,
    ) -> Result<bool, BytecodeError> {
        let code_start = code.len();
        let debug_start = debug_entries.len();
        if !expr_supported(selector) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        if !self.emit_expr(ctx, selector, code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        let mut end_jumps = Vec::new();
        for (labels, block) in branches {
            let mut label_jumps = Vec::new();
            for label in labels {
                match label {
                    crate::eval::stmt::CaseLabel::Single(value) => {
                        code.push(0x11);
                        if !self.emit_const_value(&Value::LInt(*value), code)? {
                            code.truncate(code_start);
                            debug_entries.truncate(debug_start);
                            return Ok(false);
                        }
                        code.push(0x50);
                        label_jumps.push(self.emit_jump_placeholder(code, 0x03));
                    }
                    crate::eval::stmt::CaseLabel::Range(lower, upper) => {
                        code.push(0x11);
                        if !self.emit_const_value(&Value::LInt(*lower), code)? {
                            code.truncate(code_start);
                            debug_entries.truncate(debug_start);
                            return Ok(false);
                        }
                        code.push(0x55);
                        let skip_range = self.emit_jump_placeholder(code, 0x04);
                        code.push(0x11);
                        if !self.emit_const_value(&Value::LInt(*upper), code)? {
                            code.truncate(code_start);
                            debug_entries.truncate(debug_start);
                            return Ok(false);
                        }
                        code.push(0x53);
                        label_jumps.push(self.emit_jump_placeholder(code, 0x03));
                        let after_range = code.len();
                        self.patch_jump(code, skip_range, after_range)?;
                    }
                }
            }
            let skip_branch = self.emit_jump_placeholder(code, 0x02);
            let branch_start = code.len();
            for jump in label_jumps {
                self.patch_jump(code, jump, branch_start)?;
            }
            code.push(0x12);
            if let Err(err) = self.emit_block(ctx, pou_id, block, code, debug_entries) {
                code.truncate(code_start);
                debug_entries.truncate(debug_start);
                return Err(err);
            }
            end_jumps.push(self.emit_jump_placeholder(code, 0x02));
            let next_check = code.len();
            self.patch_jump(code, skip_branch, next_check)?;
        }
        code.push(0x12);
        if let Err(err) = self.emit_block(ctx, pou_id, else_block, code, debug_entries) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Err(err);
        }
        let end = code.len();
        for jump in end_jumps {
            self.patch_jump(code, jump, end)?;
        }
        Ok(true)
    }

    fn emit_while_stmt(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        condition: &crate::eval::expr::Expr,
        body: &[crate::eval::stmt::Stmt],
        code: &mut Vec<u8>,
        debug_entries: &mut Vec<DebugEntry>,
    ) -> Result<bool, BytecodeError> {
        let code_start = code.len();
        let debug_start = debug_entries.len();
        if !expr_supported(condition) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        let loop_start = code.len();
        if !self.emit_expr(ctx, condition, code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        let jump_false = self.emit_jump_placeholder(code, 0x04);
        if let Err(err) = self.emit_block(ctx, pou_id, body, code, debug_entries) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Err(err);
        }
        let jump_back = self.emit_jump_placeholder(code, 0x02);
        self.patch_jump(code, jump_back, loop_start)?;
        let loop_end = code.len();
        self.patch_jump(code, jump_false, loop_end)?;
        Ok(true)
    }

    fn emit_repeat_stmt(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        body: &[crate::eval::stmt::Stmt],
        until: &crate::eval::expr::Expr,
        code: &mut Vec<u8>,
        debug_entries: &mut Vec<DebugEntry>,
    ) -> Result<bool, BytecodeError> {
        let code_start = code.len();
        let debug_start = debug_entries.len();
        if !expr_supported(until) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        let loop_start = code.len();
        if let Err(err) = self.emit_block(ctx, pou_id, body, code, debug_entries) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Err(err);
        }
        if !self.emit_expr(ctx, until, code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        let jump_false = self.emit_jump_placeholder(code, 0x04);
        self.patch_jump(code, jump_false, loop_start)?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_for_stmt(
        &mut self,
        ctx: &mut CodegenContext,
        pou_id: u32,
        control: &SmolStr,
        start: &crate::eval::expr::Expr,
        end: &crate::eval::expr::Expr,
        step: &crate::eval::expr::Expr,
        body: &[crate::eval::stmt::Stmt],
        code: &mut Vec<u8>,
        debug_entries: &mut Vec<DebugEntry>,
    ) -> Result<bool, BytecodeError> {
        let code_start = code.len();
        let debug_start = debug_entries.len();
        if !expr_supported(start) || !expr_supported(end) || !expr_supported(step) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        let Some((end_temp, step_temp)) = ctx.next_for_temp_pair() else {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        };
        let control_access = match self
            .resolve_lvalue_ref(ctx, &crate::eval::expr::LValue::Name(control.clone()))?
        {
            Some(reference) => AccessKind::Static(reference),
            None => match ctx.self_field_name(control) {
                Some(field) => AccessKind::SelfField(field.clone()),
                None => return Ok(false),
            },
        };
        let end_ref = match self.resolve_name_ref(ctx, &end_temp)? {
            Some(reference) => reference,
            None => return Ok(false),
        };
        let step_ref = match self.resolve_name_ref(ctx, &step_temp)? {
            Some(reference) => reference,
            None => return Ok(false),
        };

        if !self.emit_expr(ctx, start, code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        self.emit_store_access(&control_access, code)?;
        if !self.emit_expr(ctx, end, code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        self.emit_store_ref(&end_ref, code)?;
        if !self.emit_expr(ctx, step, code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        self.emit_store_ref(&step_ref, code)?;

        self.emit_load_ref(&step_ref, code)?;
        if !self.emit_const_value(&Value::LInt(0), code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        code.push(0x50);
        let jump_step_ok = self.emit_jump_placeholder(code, 0x04);
        code.push(0x01);
        let after_fault = code.len();
        self.patch_jump(code, jump_step_ok, after_fault)?;

        let loop_start = code.len();
        self.emit_load_ref(&step_ref, code)?;
        if !self.emit_const_value(&Value::LInt(0), code)? {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Ok(false);
        }
        code.push(0x55);
        let jump_true_pos = self.emit_jump_placeholder(code, 0x03);

        self.emit_load_access(&control_access, code)?;
        self.emit_load_ref(&end_ref, code)?;
        code.push(0x55);
        let jump_false_end_neg = self.emit_jump_placeholder(code, 0x04);
        let jump_to_body = self.emit_jump_placeholder(code, 0x02);

        let pos_check = code.len();
        self.patch_jump(code, jump_true_pos, pos_check)?;
        self.emit_load_access(&control_access, code)?;
        self.emit_load_ref(&end_ref, code)?;
        code.push(0x53);
        let jump_false_end_pos = self.emit_jump_placeholder(code, 0x04);

        let body_start = code.len();
        self.patch_jump(code, jump_to_body, body_start)?;
        if let Err(err) = self.emit_block(ctx, pou_id, body, code, debug_entries) {
            code.truncate(code_start);
            debug_entries.truncate(debug_start);
            return Err(err);
        }
        self.emit_load_access(&control_access, code)?;
        self.emit_load_ref(&step_ref, code)?;
        code.push(0x40);
        self.emit_store_access(&control_access, code)?;
        let jump_back = self.emit_jump_placeholder(code, 0x02);
        self.patch_jump(code, jump_back, loop_start)?;

        let loop_end = code.len();
        self.patch_jump(code, jump_false_end_neg, loop_end)?;
        self.patch_jump(code, jump_false_end_pos, loop_end)?;
        Ok(true)
    }

    fn emit_jump_placeholder(&self, code: &mut Vec<u8>, opcode: u8) -> usize {
        let pos = code.len();
        code.push(opcode);
        code.extend_from_slice(&0i32.to_le_bytes());
        pos
    }

    fn patch_jump(
        &self,
        code: &mut [u8],
        jump_pos: usize,
        target: usize,
    ) -> Result<(), BytecodeError> {
        let base = jump_pos
            .checked_add(5)
            .ok_or_else(|| BytecodeError::InvalidSection("jump base overflow".into()))?;
        let delta = target as i64 - base as i64;
        if delta < i64::from(i32::MIN) || delta > i64::from(i32::MAX) {
            return Err(BytecodeError::InvalidSection("jump offset overflow".into()));
        }
        let offset = delta as i32;
        let bytes = offset.to_le_bytes();
        let range = jump_pos + 1..jump_pos + 5;
        code[range].copy_from_slice(&bytes);
        Ok(())
    }

    fn emit_load_ref(
        &mut self,
        reference: &ValueRef,
        code: &mut Vec<u8>,
    ) -> Result<(), BytecodeError> {
        let ref_idx = self.ref_index_for(reference)?;
        code.push(0x20);
        code.extend_from_slice(&ref_idx.to_le_bytes());
        Ok(())
    }

    fn emit_store_ref(
        &mut self,
        reference: &ValueRef,
        code: &mut Vec<u8>,
    ) -> Result<(), BytecodeError> {
        let ref_idx = self.ref_index_for(reference)?;
        code.push(0x21);
        code.extend_from_slice(&ref_idx.to_le_bytes());
        Ok(())
    }

    fn emit_const_value(
        &mut self,
        value: &Value,
        code: &mut Vec<u8>,
    ) -> Result<bool, BytecodeError> {
        let const_idx = match self.const_index_for(value) {
            Ok(idx) => idx,
            Err(_) => return Ok(false),
        };
        code.push(0x10);
        code.extend_from_slice(&const_idx.to_le_bytes());
        Ok(true)
    }
}

fn expr_supported(expr: &crate::eval::expr::Expr) -> bool {
    use crate::eval::expr::Expr;
    use crate::eval::ops::{BinaryOp, UnaryOp};
    match expr {
        Expr::Literal(value) => {
            if matches!(value, Value::String(_) | Value::WString(_)) {
                return false;
            }
            type_id_for_value(value).is_some()
        }
        Expr::Name(_) => true,
        Expr::Field { target, field: _ } => matches!(target.as_ref(), Expr::Name(_)),
        Expr::Index { target, indices } => {
            matches!(target.as_ref(), Expr::Name(_)) && indices.iter().all(expr_supported)
        }
        Expr::Unary { op, expr } => {
            matches!(op, UnaryOp::Neg | UnaryOp::Not | UnaryOp::Pos) && expr_supported(expr)
        }
        Expr::Binary { op, left, right } => {
            matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow
                    | BinaryOp::And
                    | BinaryOp::Or
                    | BinaryOp::Xor
                    | BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::Lt
                    | BinaryOp::Le
                    | BinaryOp::Gt
                    | BinaryOp::Ge
            ) && expr_supported(left)
                && expr_supported(right)
        }
        _ => false,
    }
}
