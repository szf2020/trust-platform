use std::collections::{HashMap, HashSet};

use smol_str::SmolStr;

use crate::eval::{ClassDef, FunctionBlockDef, FunctionDef, MethodDef, Param};
use crate::value::Value;
use trust_hir::symbols::ParamDirection;

use crate::bytecode::{
    DebugEntry, InterfaceMethod, MethodEntry, ParamEntry, PouClassMeta, PouEntry, PouIndex, PouKind,
};

use super::util::{normalize_name, to_u32};
use super::{BytecodeEncoder, BytecodeError, CodegenContext, LocalScope};

impl<'a> BytecodeEncoder<'a> {
    pub(super) fn build_pou_index_and_bodies(
        &mut self,
    ) -> Result<(PouIndex, Vec<u8>, Vec<DebugEntry>), BytecodeError> {
        if let (Some(sources), Some(paths)) = (self.sources, self.paths) {
            if sources.len() != paths.len() {
                return Err(BytecodeError::InvalidSection(
                    "debug paths length mismatch".into(),
                ));
            }
        }
        let mut entries = Vec::new();
        let mut bodies = Vec::new();
        let mut debug_entries = Vec::new();
        let mut offset: usize = 0;

        for program in self.runtime.programs().values() {
            let id = self
                .pou_ids
                .program_id(&program.name)
                .ok_or_else(|| BytecodeError::InvalidSection("program id missing".into()))?;
            let instance_id = match self.runtime.storage().get_global(program.name.as_ref()) {
                Some(Value::Instance(id)) => Some(*id),
                _ => None,
            };
            let LocalScope {
                locals,
                local_ref_start,
                local_ref_count,
                for_temp_pairs,
            } = self.local_scope_for_body(None, &[], &program.temps, &program.body)?;
            let mut ctx = CodegenContext::new(instance_id, locals, HashMap::new(), for_temp_pairs);
            let (code, mut local_debug) = self.emit_pou_body(&mut ctx, id, &program.body)?;
            let code_offset = to_u32(offset, "POU code offset")?;
            let code_length = to_u32(code.len(), "POU code length")?;
            for entry in &mut local_debug {
                entry.code_offset =
                    entry.code_offset.checked_add(code_offset).ok_or_else(|| {
                        BytecodeError::InvalidSection("debug code offset overflow".into())
                    })?;
            }
            let mut entry = self.pou_entry_program(program, id)?;
            entry.code_offset = code_offset;
            entry.code_length = code_length;
            entry.local_ref_start = local_ref_start;
            entry.local_ref_count = local_ref_count;
            entries.push(entry);
            debug_entries.extend(local_debug);
            bodies.extend_from_slice(&code);
            offset = offset
                .checked_add(code.len())
                .ok_or_else(|| BytecodeError::InvalidSection("POU body overflow".into()))?;
        }
        for fb in self.runtime.function_blocks().values() {
            let id = self
                .pou_ids
                .function_block_id(&fb.name)
                .ok_or_else(|| BytecodeError::InvalidSection("function block id missing".into()))?;
            let LocalScope {
                locals,
                local_ref_start,
                local_ref_count,
                for_temp_pairs,
            } = self.local_scope_for_body(None, &[], &fb.temps, &fb.body)?;
            let self_fields = self.self_fields_for_owner(&fb.name)?;
            let mut ctx = CodegenContext::new(None, locals, self_fields, for_temp_pairs);
            let (code, mut local_debug) = self.emit_pou_body(&mut ctx, id, &fb.body)?;
            let code_offset = to_u32(offset, "POU code offset")?;
            let code_length = to_u32(code.len(), "POU code length")?;
            for entry in &mut local_debug {
                entry.code_offset =
                    entry.code_offset.checked_add(code_offset).ok_or_else(|| {
                        BytecodeError::InvalidSection("debug code offset overflow".into())
                    })?;
            }
            let mut entry = if self.is_stdlib_fb(&fb.name) {
                self.pou_entry_function_block(fb, id, false)?
            } else {
                self.pou_entry_function_block(fb, id, true)?
            };
            entry.code_offset = code_offset;
            entry.code_length = code_length;
            entry.local_ref_start = local_ref_start;
            entry.local_ref_count = local_ref_count;
            entries.push(entry);
            debug_entries.extend(local_debug);
            bodies.extend_from_slice(&code);
            offset = offset
                .checked_add(code.len())
                .ok_or_else(|| BytecodeError::InvalidSection("POU body overflow".into()))?;
        }
        for func in self.runtime.functions().values() {
            let id = self
                .pou_ids
                .function_id(&func.name)
                .ok_or_else(|| BytecodeError::InvalidSection("function id missing".into()))?;
            let LocalScope {
                locals,
                local_ref_start,
                local_ref_count,
                for_temp_pairs,
            } = self.local_scope_for_body(
                Some(&func.name),
                &func.params,
                &func.locals,
                &func.body,
            )?;
            let mut ctx = CodegenContext::new(None, locals, HashMap::new(), for_temp_pairs);
            let (code, mut local_debug) = self.emit_pou_body(&mut ctx, id, &func.body)?;
            let code_offset = to_u32(offset, "POU code offset")?;
            let code_length = to_u32(code.len(), "POU code length")?;
            for entry in &mut local_debug {
                entry.code_offset =
                    entry.code_offset.checked_add(code_offset).ok_or_else(|| {
                        BytecodeError::InvalidSection("debug code offset overflow".into())
                    })?;
            }
            let mut entry = self.pou_entry_function(func, id)?;
            entry.code_offset = code_offset;
            entry.code_length = code_length;
            entry.local_ref_start = local_ref_start;
            entry.local_ref_count = local_ref_count;
            entries.push(entry);
            debug_entries.extend(local_debug);
            bodies.extend_from_slice(&code);
            offset = offset
                .checked_add(code.len())
                .ok_or_else(|| BytecodeError::InvalidSection("POU body overflow".into()))?;
        }
        for class in self.runtime.classes().values() {
            let id = self
                .pou_ids
                .class_id(&class.name)
                .ok_or_else(|| BytecodeError::InvalidSection("class id missing".into()))?;
            let mut entry = self.pou_entry_class(class, id)?;
            entry.code_offset = to_u32(offset, "POU code offset")?;
            entry.code_length = 0;
            entries.push(entry);
        }
        for (owner, fb) in self.runtime.function_blocks().iter() {
            let owner_id = self
                .pou_ids
                .function_block_id(owner)
                .ok_or_else(|| BytecodeError::InvalidSection("method owner missing".into()))?;
            for method in &fb.methods {
                let id = self
                    .pou_ids
                    .method_id(owner, &method.name)
                    .ok_or_else(|| BytecodeError::InvalidSection("method id missing".into()))?;
                let LocalScope {
                    locals,
                    local_ref_start,
                    local_ref_count,
                    for_temp_pairs,
                } = self.local_scope_for_body(
                    method.return_type.as_ref().map(|_| &method.name),
                    &method.params,
                    &method.locals,
                    &method.body,
                )?;
                let self_fields = self.self_fields_for_owner(owner)?;
                let mut ctx = CodegenContext::new(None, locals, self_fields, for_temp_pairs);
                let (code, mut local_debug) = self.emit_pou_body(&mut ctx, id, &method.body)?;
                let code_offset = to_u32(offset, "POU code offset")?;
                let code_length = to_u32(code.len(), "POU code length")?;
                for entry in &mut local_debug {
                    entry.code_offset =
                        entry.code_offset.checked_add(code_offset).ok_or_else(|| {
                            BytecodeError::InvalidSection("debug code offset overflow".into())
                        })?;
                }
                let mut entry = self.pou_entry_method(method, owner_id, id)?;
                entry.code_offset = code_offset;
                entry.code_length = code_length;
                entry.local_ref_start = local_ref_start;
                entry.local_ref_count = local_ref_count;
                entries.push(entry);
                debug_entries.extend(local_debug);
                bodies.extend_from_slice(&code);
                offset = offset
                    .checked_add(code.len())
                    .ok_or_else(|| BytecodeError::InvalidSection("POU body overflow".into()))?;
            }
        }
        for (owner, class) in self.runtime.classes().iter() {
            let owner_id = self
                .pou_ids
                .class_id(owner)
                .ok_or_else(|| BytecodeError::InvalidSection("method owner missing".into()))?;
            for method in &class.methods {
                let id = self
                    .pou_ids
                    .method_id(owner, &method.name)
                    .ok_or_else(|| BytecodeError::InvalidSection("method id missing".into()))?;
                let LocalScope {
                    locals,
                    local_ref_start,
                    local_ref_count,
                    for_temp_pairs,
                } = self.local_scope_for_body(
                    method.return_type.as_ref().map(|_| &method.name),
                    &method.params,
                    &method.locals,
                    &method.body,
                )?;
                let self_fields = self.self_fields_for_owner(owner)?;
                let mut ctx = CodegenContext::new(None, locals, self_fields, for_temp_pairs);
                let (code, mut local_debug) = self.emit_pou_body(&mut ctx, id, &method.body)?;
                let code_offset = to_u32(offset, "POU code offset")?;
                let code_length = to_u32(code.len(), "POU code length")?;
                for entry in &mut local_debug {
                    entry.code_offset =
                        entry.code_offset.checked_add(code_offset).ok_or_else(|| {
                            BytecodeError::InvalidSection("debug code offset overflow".into())
                        })?;
                }
                let mut entry = self.pou_entry_method(method, owner_id, id)?;
                entry.code_offset = code_offset;
                entry.code_length = code_length;
                entry.local_ref_start = local_ref_start;
                entry.local_ref_count = local_ref_count;
                entries.push(entry);
                debug_entries.extend(local_debug);
                bodies.extend_from_slice(&code);
                offset = offset
                    .checked_add(code.len())
                    .ok_or_else(|| BytecodeError::InvalidSection("POU body overflow".into()))?;
            }
        }

        Ok((PouIndex { entries }, bodies, debug_entries))
    }

    fn pou_entry_program(
        &mut self,
        program: &crate::task::ProgramDef,
        id: u32,
    ) -> Result<PouEntry, BytecodeError> {
        let name_idx = self.strings.intern(program.name.clone());
        Ok(PouEntry {
            id,
            name_idx,
            kind: PouKind::Program,
            code_offset: 0,
            code_length: 0,
            local_ref_start: 0,
            local_ref_count: 0,
            return_type_id: None,
            owner_pou_id: None,
            params: Vec::new(),
            class_meta: None,
        })
    }

    fn pou_entry_function(
        &mut self,
        func: &FunctionDef,
        id: u32,
    ) -> Result<PouEntry, BytecodeError> {
        let name_idx = self.strings.intern(func.name.clone());
        let return_type_id = Some(self.type_index(func.return_type)?);
        let params = self.encode_params(&func.params)?;
        Ok(PouEntry {
            id,
            name_idx,
            kind: PouKind::Function,
            code_offset: 0,
            code_length: 0,
            local_ref_start: 0,
            local_ref_count: 0,
            return_type_id,
            owner_pou_id: None,
            params,
            class_meta: None,
        })
    }

    fn pou_entry_function_block(
        &mut self,
        fb: &FunctionBlockDef,
        id: u32,
        emit_params: bool,
    ) -> Result<PouEntry, BytecodeError> {
        let name_idx = self.strings.intern(fb.name.clone());
        let params = if emit_params {
            self.encode_params(&fb.params)?
        } else {
            Vec::new()
        };
        let class_meta = Some(self.class_meta(fb, None)?);
        Ok(PouEntry {
            id,
            name_idx,
            kind: PouKind::FunctionBlock,
            code_offset: 0,
            code_length: 0,
            local_ref_start: 0,
            local_ref_count: 0,
            return_type_id: None,
            owner_pou_id: None,
            params,
            class_meta,
        })
    }

    fn pou_entry_class(&mut self, class: &ClassDef, id: u32) -> Result<PouEntry, BytecodeError> {
        let name_idx = self.strings.intern(class.name.clone());
        let class_meta = Some(self.class_meta(class, None)?);
        Ok(PouEntry {
            id,
            name_idx,
            kind: PouKind::Class,
            code_offset: 0,
            code_length: 0,
            local_ref_start: 0,
            local_ref_count: 0,
            return_type_id: None,
            owner_pou_id: None,
            params: Vec::new(),
            class_meta,
        })
    }

    fn pou_entry_method(
        &mut self,
        method: &MethodDef,
        owner_id: u32,
        id: u32,
    ) -> Result<PouEntry, BytecodeError> {
        let name_idx = self.strings.intern(method.name.clone());
        let params = self.encode_params(&method.params)?;
        let return_type_id = method
            .return_type
            .map(|type_id| self.type_index(type_id))
            .transpose()?;
        Ok(PouEntry {
            id,
            name_idx,
            kind: PouKind::Method,
            code_offset: 0,
            code_length: 0,
            local_ref_start: 0,
            local_ref_count: 0,
            return_type_id,
            owner_pou_id: Some(owner_id),
            params,
            class_meta: None,
        })
    }

    pub(super) fn encode_params(
        &mut self,
        params: &[Param],
    ) -> Result<Vec<ParamEntry>, BytecodeError> {
        let mut out = Vec::with_capacity(params.len());
        for param in params {
            let name_idx = self.strings.intern(param.name.clone());
            let type_id = self.type_index(param.type_id)?;
            let direction = match param.direction {
                ParamDirection::In => 0,
                ParamDirection::Out => 1,
                ParamDirection::InOut => 2,
            };
            let default_const_idx = match (&param.default, param.direction) {
                (Some(expr), ParamDirection::In) => {
                    let value = self.const_value_from_expr(expr)?;
                    Some(self.const_index_for(&value)?)
                }
                _ => None,
            };
            out.push(ParamEntry {
                name_idx,
                type_id,
                direction,
                default_const_idx,
            });
        }
        Ok(out)
    }

    fn class_meta<T>(
        &mut self,
        def: &T,
        explicit_owner: Option<&SmolStr>,
    ) -> Result<PouClassMeta, BytecodeError>
    where
        T: ClassLike,
    {
        let owner = explicit_owner
            .cloned()
            .unwrap_or_else(|| def.name().clone());
        let parent_pou_id = def
            .base_name()
            .map(|base| {
                self.pou_ids
                    .class_like_id(&base)
                    .ok_or_else(|| BytecodeError::InvalidSection("unknown parent POU".into()))
            })
            .transpose()?;
        let methods = self.method_table_for(&owner)?;
        Ok(PouClassMeta {
            parent_pou_id,
            interfaces: Vec::new(),
            methods,
        })
    }

    fn method_table_for(&mut self, owner: &SmolStr) -> Result<Vec<MethodEntry>, BytecodeError> {
        let key = normalize_name(owner);
        if let Some(existing) = self.method_tables.get(&key) {
            return Ok(existing.clone());
        }
        if self.method_stack.contains(&key) {
            return Err(BytecodeError::InvalidSection(
                "circular inheritance detected".into(),
            ));
        }
        self.method_stack.push(key.clone());

        let (base_name, methods) = match self.class_like_def(&key) {
            Some(def) => (def.base_name(), def.methods().to_vec()),
            None => return Err(BytecodeError::InvalidSection("unknown class-like".into())),
        };

        let mut table = Vec::new();
        let mut name_to_slot: HashMap<SmolStr, usize> = HashMap::new();

        if let Some(base) = base_name {
            let base_table = self.method_table_for(&base)?;
            for entry in &base_table {
                let method_name = self
                    .strings
                    .entries
                    .get(entry.name_idx as usize)
                    .cloned()
                    .unwrap_or_default();
                name_to_slot.insert(normalize_name(&method_name), entry.vtable_slot as usize);
                table.push(entry.clone());
            }
        }

        for method in &methods {
            let name = method.name.clone();
            let name_key = normalize_name(&name);
            let pou_id = self.method_id_for(owner, method)?;
            let name_idx = self.strings.intern(method.name.clone());
            if let Some(slot) = name_to_slot.get(&name_key).copied() {
                let entry = MethodEntry {
                    name_idx,
                    pou_id,
                    vtable_slot: slot as u32,
                    access: 0,
                    flags: 0,
                };
                table[slot] = entry;
                continue;
            }
            let slot = table.len();
            let entry = MethodEntry {
                name_idx,
                pou_id,
                vtable_slot: slot as u32,
                access: 0,
                flags: 0,
            };
            name_to_slot.insert(name_key, slot);
            table.push(entry);
        }

        self.method_stack.pop();
        self.method_tables.insert(key.clone(), table.clone());
        Ok(table)
    }

    fn method_id_for(&self, owner: &SmolStr, method: &MethodDef) -> Result<u32, BytecodeError> {
        self.pou_ids
            .method_id(owner, &method.name)
            .ok_or_else(|| BytecodeError::InvalidSection("method id missing".into()))
    }

    pub(super) fn interface_methods_for(
        &mut self,
        name: &SmolStr,
    ) -> Result<Vec<InterfaceMethod>, BytecodeError> {
        let key = normalize_name(name);
        if let Some(existing) = self.interface_tables.get(&key) {
            return Ok(existing.clone());
        }
        if self.interface_stack.contains(&key) {
            return Err(BytecodeError::InvalidSection(
                "circular interface inheritance detected".into(),
            ));
        }
        self.interface_stack.push(key.clone());

        let def = self
            .runtime
            .interfaces()
            .get(&key)
            .ok_or_else(|| BytecodeError::InvalidSection("unknown interface".into()))?;
        let base = def.base.clone();
        let methods = def.methods.clone();

        let mut table = Vec::new();
        let mut name_to_slot: HashMap<SmolStr, u32> = HashMap::new();

        if let Some(base_name) = base {
            let base_methods = self.interface_methods_for(&base_name)?;
            for method in &base_methods {
                let method_name = self
                    .strings
                    .entries
                    .get(method.name_idx as usize)
                    .cloned()
                    .unwrap_or_default();
                name_to_slot.insert(normalize_name(&method_name), method.slot);
                table.push(method.clone());
            }
        }

        for method in &methods {
            let name_idx = self.strings.intern(method.name.clone());
            let name_key = normalize_name(&method.name);
            if name_to_slot.contains_key(&name_key) {
                continue;
            }
            let slot = table.len() as u32;
            table.push(InterfaceMethod { name_idx, slot });
            name_to_slot.insert(name_key, slot);
        }

        self.interface_stack.pop();
        self.interface_tables.insert(key.clone(), table.clone());
        Ok(table)
    }

    fn class_like_def(&self, key: &SmolStr) -> Option<ClassLikeDef<'_>> {
        if let Some(fb) = self.runtime.function_blocks().get(key) {
            Some(ClassLikeDef::FunctionBlock(fb))
        } else {
            self.runtime.classes().get(key).map(ClassLikeDef::Class)
        }
    }

    fn self_fields_for_owner(
        &self,
        owner: &SmolStr,
    ) -> Result<HashMap<SmolStr, SmolStr>, BytecodeError> {
        let mut fields = HashMap::new();
        let mut seen = HashSet::new();
        let mut current = Some(owner.clone());
        while let Some(name) = current {
            let key = normalize_name(&name);
            if !seen.insert(key.clone()) {
                return Err(BytecodeError::InvalidSection(
                    "circular inheritance detected".into(),
                ));
            }
            let def = self
                .class_like_def(&key)
                .ok_or_else(|| BytecodeError::InvalidSection("unknown class-like".into()))?;
            match def {
                ClassLikeDef::FunctionBlock(fb) => {
                    for param in &fb.params {
                        insert_self_field(&mut fields, &param.name);
                    }
                    for var in &fb.vars {
                        insert_self_field(&mut fields, &var.name);
                    }
                    current = def.base_name();
                }
                ClassLikeDef::Class(class) => {
                    for var in &class.vars {
                        insert_self_field(&mut fields, &var.name);
                    }
                    current = class.base.clone();
                }
            }
        }
        Ok(fields)
    }
}

trait ClassLike {
    fn name(&self) -> &SmolStr;
    fn base_name(&self) -> Option<SmolStr>;
    fn methods(&self) -> &[MethodDef];
}

impl ClassLike for FunctionBlockDef {
    fn name(&self) -> &SmolStr {
        &self.name
    }

    fn base_name(&self) -> Option<SmolStr> {
        self.base.as_ref().map(|base| match base {
            crate::eval::FunctionBlockBase::FunctionBlock(name)
            | crate::eval::FunctionBlockBase::Class(name) => name.clone(),
        })
    }

    fn methods(&self) -> &[MethodDef] {
        &self.methods
    }
}

impl ClassLike for ClassDef {
    fn name(&self) -> &SmolStr {
        &self.name
    }

    fn base_name(&self) -> Option<SmolStr> {
        self.base.clone()
    }

    fn methods(&self) -> &[MethodDef] {
        &self.methods
    }
}

enum ClassLikeDef<'a> {
    FunctionBlock(&'a FunctionBlockDef),
    Class(&'a ClassDef),
}

impl<'a> ClassLikeDef<'a> {
    fn base_name(&self) -> Option<SmolStr> {
        match self {
            ClassLikeDef::FunctionBlock(def) => def.base_name(),
            ClassLikeDef::Class(def) => def.base_name(),
        }
    }

    fn methods(&self) -> &[MethodDef] {
        match self {
            ClassLikeDef::FunctionBlock(def) => def.methods(),
            ClassLikeDef::Class(def) => def.methods(),
        }
    }
}

fn insert_self_field(map: &mut HashMap<SmolStr, SmolStr>, name: &SmolStr) {
    let key = normalize_name(name);
    map.entry(key).or_insert_with(|| name.clone());
}
