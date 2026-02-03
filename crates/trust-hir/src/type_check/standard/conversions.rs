use super::super::calls::{BoundArgs, CallArg};
use super::super::literals::is_untyped_real_literal_expr;
use super::super::*;
use super::helpers::builtin_param;

impl<'a, 'b> StandardChecker<'a, 'b> {
    pub(in crate::type_check) fn infer_conversion_function_call(
        &mut self,
        name: &str,
        node: &SyntaxNode,
    ) -> Option<TypeId> {
        let upper = name;

        if upper.eq_ignore_ascii_case("TRUNC") {
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.is_real_type(arg_type) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected REAL or LREAL input",
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(TypeId::DINT);
        }

        if let Some(dst_name) = upper.strip_prefix("TRUNC_") {
            let dst = TypeId::from_builtin_name(dst_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.is_real_type(arg_type) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected REAL or LREAL input",
                );
                return Some(TypeId::UNKNOWN);
            }
            if !self.is_integer_type(dst) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    format!("invalid TRUNC target '{}'", self.checker.type_name(dst)),
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(dst);
        }

        if let Some((src_name, dst_name)) = upper.split_once("_TRUNC_") {
            let src = TypeId::from_builtin_name(src_name)?;
            let dst = TypeId::from_builtin_name(dst_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.expect_assignable_in_param(src, &arg, arg_type) {
                return Some(TypeId::UNKNOWN);
            }
            if !self.is_real_type(src) || !self.is_integer_type(dst) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "invalid TRUNC conversion",
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(dst);
        }

        if let Some(dst_name) = upper.strip_prefix("TO_BCD_") {
            let dst = TypeId::from_builtin_name(dst_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.is_unsigned_int_type(arg_type) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected unsigned integer input",
                );
                return Some(TypeId::UNKNOWN);
            }
            if !self.is_bit_string_type(dst) || dst == TypeId::BOOL {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    format!("invalid BCD target '{}'", self.checker.type_name(dst)),
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(dst);
        }

        if let Some((dst_name, src_name)) = upper.split_once("_TO_BCD_") {
            let dst = TypeId::from_builtin_name(dst_name)?;
            let src = TypeId::from_builtin_name(src_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.expect_assignable_in_param(dst, &arg, arg_type) {
                return Some(TypeId::UNKNOWN);
            }
            if !self.is_unsigned_int_type(dst)
                || !self.is_bit_string_type(src)
                || src == TypeId::BOOL
            {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "invalid BCD conversion",
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(src);
        }

        if let Some(dst_name) = upper.strip_prefix("BCD_TO_") {
            let dst = TypeId::from_builtin_name(dst_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.is_bit_string_type(arg_type) || arg_type == TypeId::BOOL {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected BYTE/WORD/DWORD/LWORD input",
                );
                return Some(TypeId::UNKNOWN);
            }
            if !self.is_unsigned_int_type(dst) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    format!("invalid BCD target '{}'", self.checker.type_name(dst)),
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(dst);
        }

        if let Some((src_name, dst_name)) = upper.split_once("_BCD_TO_") {
            let src = TypeId::from_builtin_name(src_name)?;
            let dst = TypeId::from_builtin_name(dst_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.expect_assignable_in_param(src, &arg, arg_type) {
                return Some(TypeId::UNKNOWN);
            }
            if !self.is_bit_string_type(src)
                || src == TypeId::BOOL
                || !self.is_unsigned_int_type(dst)
            {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "invalid BCD conversion",
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(dst);
        }

        if let Some(dst_name) = upper.strip_prefix("TO_") {
            let dst = TypeId::from_builtin_name(dst_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.is_conversion_allowed(arg_type, dst) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    format!(
                        "cannot convert '{}' to '{}'",
                        self.checker.type_name(arg_type),
                        self.checker.type_name(dst)
                    ),
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(dst);
        }

        if let Some((src_name, dst_name)) = upper.split_once("_TO_") {
            let src = TypeId::from_builtin_name(src_name)?;
            let dst = TypeId::from_builtin_name(dst_name)?;
            let Some((arg, arg_type)) = self.collect_single_conversion_arg(node) else {
                return Some(TypeId::UNKNOWN);
            };
            if !self.expect_assignable_in_param(src, &arg, arg_type) {
                return Some(TypeId::UNKNOWN);
            }
            if !self.is_conversion_allowed(src, dst) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    format!(
                        "cannot convert '{}' to '{}'",
                        self.checker.type_name(src),
                        self.checker.type_name(dst)
                    ),
                );
                return Some(TypeId::UNKNOWN);
            }
            return Some(dst);
        }

        None
    }

    fn collect_single_conversion_arg(&mut self, node: &SyntaxNode) -> Option<(CallArg, TypeId)> {
        let params = vec![builtin_param("IN", ParamDirection::In)];
        let call = self.builtin_call(node, params);
        call.check_formal_arg_count(self, node, 1);
        call.arg(0)
    }

    fn expect_assignable_in_param(
        &mut self,
        expected: TypeId,
        arg: &CallArg,
        arg_type: TypeId,
    ) -> bool {
        if !self.checker.is_assignable(expected, arg_type) {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidArgumentType,
                arg.range,
                format!(
                    "expected '{}' for parameter 'IN'",
                    self.checker.type_name(expected)
                ),
            );
            return false;
        }
        true
    }

    pub(in crate::type_check) fn check_formal_arg_count(
        &mut self,
        bound: &BoundArgs,
        node: &SyntaxNode,
        actual: usize,
        expected: usize,
    ) {
        if bound.formal_call && actual != expected {
            self.checker.diagnostics.error(
                DiagnosticCode::WrongArgumentCount,
                node.text_range(),
                format!("expected {} arguments, found {}", expected, actual),
            );
        }
    }

    pub(in crate::type_check) fn base_type_id(&self, type_id: TypeId) -> TypeId {
        let resolved = self.checker.resolve_alias_type(type_id);
        self.checker.resolve_subrange_base(resolved)
    }

    pub(in crate::type_check) fn is_numeric_type(&self, type_id: TypeId) -> bool {
        self.checker
            .resolved_type(type_id)
            .is_some_and(|ty| ty.is_numeric())
    }

    pub(in crate::type_check) fn is_integer_type(&self, type_id: TypeId) -> bool {
        self.checker
            .resolved_type(type_id)
            .is_some_and(|ty| ty.is_integer())
    }

    pub(in crate::type_check) fn is_unsigned_int_type(&self, type_id: TypeId) -> bool {
        matches!(
            self.base_type_id(type_id),
            TypeId::USINT | TypeId::UINT | TypeId::UDINT | TypeId::ULINT
        )
    }

    pub(in crate::type_check) fn is_real_type(&self, type_id: TypeId) -> bool {
        self.checker
            .resolved_type(type_id)
            .is_some_and(|ty| ty.is_float())
    }

    pub(in crate::type_check) fn is_bit_string_type(&self, type_id: TypeId) -> bool {
        self.checker
            .resolved_type(type_id)
            .is_some_and(|ty| ty.is_bit_string())
    }

    pub(in crate::type_check) fn is_string_type(&self, type_id: TypeId) -> bool {
        self.checker
            .resolved_type(type_id)
            .is_some_and(|ty| ty.is_string())
    }

    pub(in crate::type_check) fn string_kind(&self, type_id: TypeId) -> Option<bool> {
        match self.checker.resolved_type(type_id)? {
            Type::String { .. } => Some(false),
            Type::WString { .. } => Some(true),
            _ => None,
        }
    }

    pub(in crate::type_check) fn normalize_string_type_id(&self, type_id: TypeId) -> TypeId {
        match self.string_kind(type_id) {
            Some(true) => TypeId::WSTRING,
            Some(false) => TypeId::STRING,
            None => type_id,
        }
    }

    pub(in crate::type_check) fn is_time_related_type(&self, type_id: TypeId) -> bool {
        self.checker
            .resolved_type(type_id)
            .is_some_and(|ty| ty.is_time())
    }

    pub(in crate::type_check) fn is_time_duration_type(&self, type_id: TypeId) -> bool {
        matches!(self.base_type_id(type_id), TypeId::TIME | TypeId::LTIME)
    }

    pub(in crate::type_check) fn is_elementary_type(&self, type_id: TypeId) -> bool {
        let resolved = self.base_type_id(type_id);
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
            )
        )
    }

    pub(in crate::type_check) fn common_numeric_type_for_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> Option<TypeId> {
        let mut common: Option<TypeId> = None;
        let mut saw_untyped_real = false;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if !self.is_numeric_type(base) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected numeric type",
                );
                return None;
            }
            if is_untyped_real_literal_expr(&arg.expr) {
                saw_untyped_real = true;
                continue;
            }
            common = Some(match common {
                None => base,
                Some(current) => self.checker.wider_numeric(current, base),
            });
        }
        let common = match common {
            Some(base) => {
                if saw_untyped_real {
                    let base_ty = self.checker.symbols.type_by_id(base);
                    if base_ty.is_some_and(|ty| ty.is_float()) {
                        base
                    } else {
                        TypeId::LREAL
                    }
                } else {
                    base
                }
            }
            None => {
                if saw_untyped_real {
                    TypeId::LREAL
                } else {
                    return None;
                }
            }
        };
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if is_untyped_real_literal_expr(&arg.expr) && common == TypeId::REAL {
                continue;
            }
            if base != common {
                self.checker
                    .warn_implicit_conversion(common, base, arg.range);
            }
        }
        Some(common)
    }

    pub(in crate::type_check) fn common_integer_type_for_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> Option<TypeId> {
        let mut common: Option<TypeId> = None;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if !self.is_integer_type(base) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected integer type",
                );
                return None;
            }
            common = Some(match common {
                None => base,
                Some(current) => self.checker.wider_numeric(current, base),
            });
        }
        let common = common?;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if base != common {
                self.checker
                    .warn_implicit_conversion(common, base, arg.range);
            }
        }
        Some(common)
    }

    pub(in crate::type_check) fn common_real_type_for_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> Option<TypeId> {
        let mut any_lreal = false;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if !self.is_real_type(base) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected REAL or LREAL type",
                );
                return None;
            }
            if base == TypeId::LREAL {
                any_lreal = true;
            }
        }
        let common = if any_lreal {
            TypeId::LREAL
        } else {
            TypeId::REAL
        };
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if base != common {
                self.checker
                    .warn_implicit_conversion(common, base, arg.range);
            }
        }
        Some(common)
    }

    pub(in crate::type_check) fn common_bit_type_for_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> Option<TypeId> {
        let mut common: Option<(TypeId, u32)> = None;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if !self.is_bit_string_type(base) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected bit string type",
                );
                return None;
            }
            let Some(size) = self.checker.resolved_type(base).and_then(Type::bit_size) else {
                continue;
            };
            common = Some(match common {
                None => (base, size),
                Some((current, current_size)) => {
                    if size > current_size {
                        (base, size)
                    } else {
                        (current, current_size)
                    }
                }
            });
        }
        let (common, _) = common?;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if base != common {
                self.checker
                    .warn_implicit_conversion(common, base, arg.range);
            }
        }
        Some(common)
    }

    pub(in crate::type_check) fn common_string_type_for_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> Option<TypeId> {
        let mut common: Option<TypeId> = None;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if !self.is_string_type(base) {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "expected STRING or WSTRING type",
                );
                return None;
            }
            match common {
                None => common = Some(base),
                Some(current) => {
                    if self.is_string_type(current) && self.is_string_type(base) {
                        let current_is_wide = self.string_kind(current);
                        let base_is_wide = self.string_kind(base);
                        if current_is_wide != base_is_wide {
                            self.checker.diagnostics.error(
                                DiagnosticCode::InvalidArgumentType,
                                arg.range,
                                "cannot mix STRING and WSTRING",
                            );
                            return None;
                        }
                    }
                }
            }
        }
        let common = common?;
        for (arg, ty) in args {
            let base = self.base_type_id(*ty);
            if base != common {
                self.checker
                    .warn_implicit_conversion(common, base, arg.range);
            }
        }
        Some(common)
    }

    pub(in crate::type_check) fn common_any_type_for_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> Option<TypeId> {
        if args.is_empty() {
            return None;
        }
        if args.iter().all(|(_, ty)| self.is_elementary_type(*ty)) {
            return self.common_elementary_type_for_args(args);
        }

        let base = self.base_type_id(args[0].1);
        for (arg, ty) in args.iter().skip(1) {
            let other = self.base_type_id(*ty);
            if base != other {
                self.checker.diagnostics.error(
                    DiagnosticCode::InvalidArgumentType,
                    arg.range,
                    "arguments must have the same type",
                );
                return None;
            }
        }
        Some(base)
    }

    pub(in crate::type_check) fn common_elementary_type_for_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> Option<TypeId> {
        if args.is_empty() {
            return None;
        }

        if args.iter().all(|(_, ty)| self.is_numeric_type(*ty)) {
            return self.common_numeric_type_for_args(args);
        }

        if args.iter().all(|(_, ty)| self.is_bit_string_type(*ty)) {
            return self.common_bit_type_for_args(args);
        }

        if args.iter().all(|(_, ty)| self.is_string_type(*ty)) {
            return self.common_string_type_for_args(args);
        }

        if args.iter().all(|(_, ty)| self.is_time_related_type(*ty)) {
            let base = self.base_type_id(args[0].1);
            for (arg, ty) in args.iter().skip(1) {
                let other = self.base_type_id(*ty);
                if base != other {
                    self.checker.diagnostics.error(
                        DiagnosticCode::InvalidArgumentType,
                        arg.range,
                        "time/date arguments must have the same type",
                    );
                    return None;
                }
            }
            return Some(base);
        }

        if args
            .iter()
            .all(|(_, ty)| matches!(self.checker.resolved_type(*ty), Some(Type::Enum { .. })))
        {
            let base = self.base_type_id(args[0].1);
            for (arg, ty) in args.iter().skip(1) {
                let other = self.base_type_id(*ty);
                if base != other {
                    self.checker.diagnostics.error(
                        DiagnosticCode::InvalidArgumentType,
                        arg.range,
                        "enum arguments must have the same type",
                    );
                    return None;
                }
            }
            return Some(base);
        }

        for (arg, _) in args {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidArgumentType,
                arg.range,
                "expected elementary type",
            );
        }
        None
    }

    pub(in crate::type_check) fn check_comparable_args(
        &mut self,
        args: &[(CallArg, TypeId)],
    ) -> bool {
        if args.len() < 2 {
            return true;
        }

        if args.iter().all(|(_, ty)| self.is_numeric_type(*ty)) {
            self.common_numeric_type_for_args(args);
            return true;
        }

        if args.iter().all(|(_, ty)| self.is_bit_string_type(*ty)) {
            self.common_bit_type_for_args(args);
            return true;
        }

        if args.iter().all(|(_, ty)| self.is_string_type(*ty)) {
            self.common_string_type_for_args(args);
            return true;
        }

        if args.iter().all(|(_, ty)| self.is_time_related_type(*ty)) {
            let base = self.base_type_id(args[0].1);
            for (arg, ty) in args.iter().skip(1) {
                if self.base_type_id(*ty) != base {
                    self.checker.diagnostics.error(
                        DiagnosticCode::InvalidArgumentType,
                        arg.range,
                        "time/date arguments must have the same type",
                    );
                    return false;
                }
            }
            return true;
        }

        if args
            .iter()
            .all(|(_, ty)| matches!(self.checker.resolved_type(*ty), Some(Type::Enum { .. })))
        {
            let base = self.base_type_id(args[0].1);
            for (arg, ty) in args.iter().skip(1) {
                if self.base_type_id(*ty) != base {
                    self.checker.diagnostics.error(
                        DiagnosticCode::InvalidArgumentType,
                        arg.range,
                        "enum arguments must have the same type",
                    );
                    return false;
                }
            }
            return true;
        }

        for (arg, _) in args {
            self.checker.diagnostics.error(
                DiagnosticCode::InvalidArgumentType,
                arg.range,
                "arguments are not comparable",
            );
        }
        false
    }

    pub(in crate::type_check) fn is_conversion_allowed(&self, src: TypeId, dst: TypeId) -> bool {
        let src = self.base_type_id(src);
        let dst = self.base_type_id(dst);

        if src == dst {
            return true;
        }

        if self.is_numeric_type(src) && self.is_numeric_type(dst) {
            return true;
        }

        if matches!(
            src,
            TypeId::BYTE | TypeId::WORD | TypeId::DWORD | TypeId::LWORD
        ) && matches!(
            dst,
            TypeId::BYTE | TypeId::WORD | TypeId::DWORD | TypeId::LWORD
        ) {
            return true;
        }

        if matches!(
            src,
            TypeId::BOOL | TypeId::BYTE | TypeId::WORD | TypeId::DWORD | TypeId::LWORD
        ) && matches!(
            dst,
            TypeId::SINT
                | TypeId::INT
                | TypeId::DINT
                | TypeId::LINT
                | TypeId::USINT
                | TypeId::UINT
                | TypeId::UDINT
                | TypeId::ULINT
        ) {
            return true;
        }

        if matches!(src, TypeId::DWORD) && dst == TypeId::REAL {
            return true;
        }
        if matches!(src, TypeId::LWORD) && dst == TypeId::LREAL {
            return true;
        }

        if matches!(
            dst,
            TypeId::BYTE | TypeId::WORD | TypeId::DWORD | TypeId::LWORD
        ) && matches!(
            src,
            TypeId::SINT
                | TypeId::INT
                | TypeId::DINT
                | TypeId::LINT
                | TypeId::USINT
                | TypeId::UINT
                | TypeId::UDINT
                | TypeId::ULINT
        ) {
            return true;
        }

        if src == TypeId::REAL && dst == TypeId::DWORD {
            return true;
        }
        if src == TypeId::LREAL && dst == TypeId::LWORD {
            return true;
        }

        if matches!(src, TypeId::LTIME) && dst == TypeId::TIME {
            return true;
        }
        if matches!(src, TypeId::TIME) && dst == TypeId::LTIME {
            return true;
        }
        if matches!(src, TypeId::LDT) && dst == TypeId::DT {
            return true;
        }
        if matches!(src, TypeId::LDT) && dst == TypeId::DATE {
            return true;
        }
        if matches!(src, TypeId::LDT) && dst == TypeId::LTOD {
            return true;
        }
        if matches!(src, TypeId::LDT) && dst == TypeId::TOD {
            return true;
        }
        if matches!(src, TypeId::DT) && dst == TypeId::LDT {
            return true;
        }
        if matches!(src, TypeId::DT) && dst == TypeId::DATE {
            return true;
        }
        if matches!(src, TypeId::DT) && dst == TypeId::LTOD {
            return true;
        }
        if matches!(src, TypeId::DT) && dst == TypeId::TOD {
            return true;
        }
        if matches!(src, TypeId::LTOD) && dst == TypeId::TOD {
            return true;
        }
        if matches!(src, TypeId::TOD) && dst == TypeId::LTOD {
            return true;
        }

        let src = self.normalize_string_type_id(src);
        let dst = self.normalize_string_type_id(dst);

        if matches!(src, TypeId::WSTRING) && matches!(dst, TypeId::STRING | TypeId::WCHAR) {
            return true;
        }
        if matches!(src, TypeId::STRING) && matches!(dst, TypeId::WSTRING | TypeId::CHAR) {
            return true;
        }
        if matches!(src, TypeId::WCHAR) && matches!(dst, TypeId::WSTRING | TypeId::CHAR) {
            return true;
        }
        if matches!(src, TypeId::CHAR) && matches!(dst, TypeId::STRING | TypeId::WCHAR) {
            return true;
        }

        false
    }
}
