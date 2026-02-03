use indexmap::IndexMap;
use smol_str::SmolStr;

use crate::eval::FunctionBlockDef;
use crate::io::IoAddress;
use crate::memory::{InstanceId, VariableStorage};
use crate::task::ProgramDef;
use crate::value::Value;
use trust_hir::types::TypeRegistry;
use trust_hir::{Type, TypeId};

use super::{CompileError, WildcardRequirement};

#[derive(Debug, Clone)]
struct IoLeafBinding {
    reference: crate::value::ValueRef,
    offset_bytes: u64,
    bit_offset: u8,
    size: crate::io::IoSize,
    value_type: TypeId,
}

#[derive(Debug, Clone)]
pub(super) struct InstanceBinding {
    pub(super) reference: crate::value::ValueRef,
    pub(super) type_id: TypeId,
    pub(super) address: IoAddress,
    pub(super) display_name: SmolStr,
}

#[derive(Debug, Clone)]
enum FieldAddress {
    Relative { offset_bytes: u64, bit_offset: u8 },
    Absolute(IoAddress),
}

pub(super) fn bind_value_ref_to_address(
    io: &mut crate::io::IoInterface,
    registry: &TypeRegistry,
    reference: crate::value::ValueRef,
    type_id: TypeId,
    address: &IoAddress,
    display_name: Option<SmolStr>,
) -> Result<(), CompileError> {
    let mut bindings = Vec::new();
    collect_io_bindings(registry, type_id, reference, 0, 0, &mut bindings)?;
    for binding in bindings {
        let target = offset_address(
            address,
            binding.offset_bytes,
            binding.size,
            binding.bit_offset,
        )?;
        if let Some(name) = display_name.clone() {
            io.bind_ref_named_typed(binding.reference, target, binding.value_type, name);
        } else {
            io.bind_ref_typed(binding.reference, target, binding.value_type);
        }
    }
    Ok(())
}

pub(super) fn join_instance_path(prefix: &SmolStr, name: &SmolStr) -> SmolStr {
    if prefix.is_empty() {
        name.clone()
    } else {
        SmolStr::new(format!("{prefix}.{name}"))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn collect_instance_bindings(
    registry: &TypeRegistry,
    storage: &VariableStorage,
    function_blocks: &IndexMap<SmolStr, FunctionBlockDef>,
    instance_id: InstanceId,
    instance_name: &SmolStr,
    wildcards: &mut Vec<WildcardRequirement>,
    visited: &mut std::collections::HashSet<InstanceId>,
    out: &mut Vec<InstanceBinding>,
) -> Result<(), CompileError> {
    if !visited.insert(instance_id) {
        return Ok(());
    }
    let instance = storage
        .get_instance(instance_id)
        .ok_or_else(|| CompileError::new("invalid function block instance"))?;
    let key = SmolStr::new(instance.type_name.to_ascii_uppercase());
    let Some(fb) = function_blocks.get(&key) else {
        return Ok(());
    };

    for param in &fb.params {
        let Some(address) = &param.address else {
            let reference = storage
                .ref_for_instance(instance_id, param.name.as_ref())
                .ok_or_else(|| CompileError::new("invalid function block parameter"))?;
            let full_name = join_instance_path(instance_name, &param.name);
            collect_direct_field_bindings(
                registry,
                &reference,
                param.type_id,
                &full_name,
                wildcards,
                out,
            )?;
            continue;
        };
        let reference = storage
            .ref_for_instance(instance_id, param.name.as_ref())
            .ok_or_else(|| CompileError::new("invalid function block parameter"))?;
        let full_name = join_instance_path(instance_name, &param.name);
        if address.wildcard {
            wildcards.push(WildcardRequirement {
                name: full_name,
                reference,
                area: address.area,
            });
        } else {
            out.push(InstanceBinding {
                reference,
                type_id: param.type_id,
                address: address.clone(),
                display_name: full_name,
            });
        }
    }

    for var in &fb.vars {
        let Some(address) = &var.address else {
            let reference = storage
                .ref_for_instance(instance_id, var.name.as_ref())
                .ok_or_else(|| CompileError::new("invalid function block variable"))?;
            let full_name = join_instance_path(instance_name, &var.name);
            collect_direct_field_bindings(
                registry,
                &reference,
                var.type_id,
                &full_name,
                wildcards,
                out,
            )?;
            continue;
        };
        let reference = storage
            .ref_for_instance(instance_id, var.name.as_ref())
            .ok_or_else(|| CompileError::new("invalid function block variable"))?;
        let full_name = join_instance_path(instance_name, &var.name);
        if address.wildcard {
            wildcards.push(WildcardRequirement {
                name: full_name,
                reference,
                area: address.area,
            });
        } else {
            out.push(InstanceBinding {
                reference,
                type_id: var.type_id,
                address: address.clone(),
                display_name: full_name,
            });
        }
    }

    for (name, value) in instance.variables.iter() {
        let Value::Instance(nested_id) = value else {
            continue;
        };
        let nested_name = join_instance_path(instance_name, name);
        collect_instance_bindings(
            registry,
            storage,
            function_blocks,
            *nested_id,
            &nested_name,
            wildcards,
            visited,
            out,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn collect_program_instance_bindings(
    registry: &TypeRegistry,
    storage: &VariableStorage,
    function_blocks: &IndexMap<SmolStr, FunctionBlockDef>,
    program: &ProgramDef,
    instance_id: InstanceId,
    instance_name: &SmolStr,
    wildcards: &mut Vec<WildcardRequirement>,
    visited: &mut std::collections::HashSet<InstanceId>,
    out: &mut Vec<InstanceBinding>,
) -> Result<(), CompileError> {
    for var in &program.vars {
        let reference = storage
            .ref_for_instance(instance_id, var.name.as_ref())
            .ok_or_else(|| CompileError::new("invalid program variable reference"))?;
        let full_name = join_instance_path(instance_name, &var.name);
        if let Some(address) = &var.address {
            if address.wildcard {
                wildcards.push(WildcardRequirement {
                    name: full_name,
                    reference,
                    area: address.area,
                });
            } else {
                out.push(InstanceBinding {
                    reference,
                    type_id: var.type_id,
                    address: address.clone(),
                    display_name: full_name,
                });
            }
            continue;
        }
        collect_direct_field_bindings(
            registry,
            &reference,
            var.type_id,
            &full_name,
            wildcards,
            out,
        )?;
    }

    let instance = storage
        .get_instance(instance_id)
        .ok_or_else(|| CompileError::new("invalid program instance"))?;
    for (name, value) in instance.variables.iter() {
        let Value::Instance(nested_id) = value else {
            continue;
        };
        let nested_name = join_instance_path(instance_name, name);
        collect_instance_bindings(
            registry,
            storage,
            function_blocks,
            *nested_id,
            &nested_name,
            wildcards,
            visited,
            out,
        )?;
    }

    Ok(())
}

fn parse_field_address(text: &str) -> Result<FieldAddress, CompileError> {
    let trimmed = text.trim();
    if !trimmed.starts_with('%') || trimmed.len() < 2 {
        return Err(CompileError::new("invalid direct address"));
    }
    let mut chars = trimmed[1..].chars();
    let Some(prefix) = chars.next() else {
        return Err(CompileError::new("invalid direct address"));
    };
    match prefix {
        'I' | 'Q' | 'M' => {
            let address = IoAddress::parse(trimmed)
                .map_err(|err| CompileError::new(format!("invalid I/O address: {err}")))?;
            Ok(FieldAddress::Absolute(address))
        }
        'X' | 'B' | 'W' | 'D' | 'L' => parse_relative_address(trimmed),
        _ => Err(CompileError::new("invalid direct address")),
    }
}

fn parse_relative_address(text: &str) -> Result<FieldAddress, CompileError> {
    let trimmed = text.trim();
    if !trimmed.starts_with('%') || trimmed.len() < 3 {
        return Err(CompileError::new("invalid relative address"));
    }
    let mut chars = trimmed[1..].chars();
    let Some(size) = chars.next() else {
        return Err(CompileError::new("invalid relative address"));
    };
    let rest = chars.as_str();
    if rest.is_empty() {
        return Err(CompileError::new("invalid relative address"));
    }
    match size {
        'X' => {
            let mut parts = rest.split('.');
            let byte_part = parts
                .next()
                .ok_or_else(|| CompileError::new("invalid relative address"))?;
            let byte = byte_part
                .parse::<u64>()
                .map_err(|_| CompileError::new("invalid relative address"))?;
            let bit = match parts.next() {
                Some(bit_part) if !bit_part.is_empty() => bit_part
                    .parse::<u8>()
                    .map_err(|_| CompileError::new("invalid relative address"))?,
                _ => 0,
            };
            if bit > 7 || parts.next().is_some() {
                return Err(CompileError::new("invalid relative address"));
            }
            Ok(FieldAddress::Relative {
                offset_bytes: byte,
                bit_offset: bit,
            })
        }
        'B' | 'W' | 'D' | 'L' => {
            if rest.contains('.') {
                return Err(CompileError::new("invalid relative address"));
            }
            let byte = rest
                .parse::<u64>()
                .map_err(|_| CompileError::new("invalid relative address"))?;
            Ok(FieldAddress::Relative {
                offset_bytes: byte,
                bit_offset: 0,
            })
        }
        _ => Err(CompileError::new("invalid relative address")),
    }
}

fn collect_io_bindings(
    registry: &TypeRegistry,
    type_id: TypeId,
    reference: crate::value::ValueRef,
    offset_bytes: u64,
    bit_offset: u8,
    out: &mut Vec<IoLeafBinding>,
) -> Result<(), CompileError> {
    let ty = registry
        .get(type_id)
        .ok_or_else(|| CompileError::new("unknown type for I/O binding"))?;
    match ty {
        Type::Alias { target, .. } => {
            collect_io_bindings(registry, *target, reference, offset_bytes, bit_offset, out)
        }
        Type::Subrange { base, .. } => {
            collect_io_bindings(registry, *base, reference, offset_bytes, bit_offset, out)
        }
        Type::Enum { base, .. } => {
            collect_io_bindings(registry, *base, reference, offset_bytes, bit_offset, out)
        }
        Type::Array {
            element,
            dimensions,
        } => {
            let element_size = type_size_bytes(*element, registry)?;
            let lengths: Vec<i64> = dimensions
                .iter()
                .map(|(lower, upper)| upper - lower + 1)
                .collect();
            if lengths.iter().any(|len| *len <= 0) {
                return Err(CompileError::new("invalid array bounds for I/O binding"));
            }
            let mut strides = vec![element_size; lengths.len()];
            let mut stride = element_size;
            for idx in (0..lengths.len()).rev() {
                strides[idx] = stride;
                stride = stride
                    .checked_mul(
                        u64::try_from(lengths[idx]).map_err(|_| {
                            CompileError::new("array length overflow for I/O binding")
                        })?,
                    )
                    .ok_or_else(|| CompileError::new("array stride overflow for I/O binding"))?;
            }

            #[allow(clippy::too_many_arguments)]
            fn walk_array(
                registry: &TypeRegistry,
                element: TypeId,
                dimensions: &[(i64, i64)],
                lengths: &[i64],
                strides: &[u64],
                reference: &crate::value::ValueRef,
                offset_bytes: u64,
                current_dim: usize,
                indices: &mut Vec<i64>,
                bit_offset: u8,
                out: &mut Vec<IoLeafBinding>,
            ) -> Result<(), CompileError> {
                if current_dim == dimensions.len() {
                    let mut ref_with_index = reference.clone();
                    ref_with_index
                        .path
                        .push(crate::value::RefSegment::Index(indices.clone()));
                    return collect_io_bindings(
                        registry,
                        element,
                        ref_with_index,
                        offset_bytes,
                        bit_offset,
                        out,
                    );
                }
                let (lower, _upper) = dimensions[current_dim];
                let stride = strides[current_dim];
                let len = lengths[current_dim];
                for idx in 0..len {
                    let index_value = lower + idx;
                    let offset = stride
                        .checked_mul(u64::try_from(idx).map_err(|_| {
                            CompileError::new("array offset overflow for I/O binding")
                        })?)
                        .ok_or_else(|| {
                            CompileError::new("array offset overflow for I/O binding")
                        })?;
                    let total_offset = offset_bytes.checked_add(offset).ok_or_else(|| {
                        CompileError::new("array offset overflow for I/O binding")
                    })?;
                    indices.push(index_value);
                    walk_array(
                        registry,
                        element,
                        dimensions,
                        lengths,
                        strides,
                        reference,
                        total_offset,
                        current_dim + 1,
                        indices,
                        bit_offset,
                        out,
                    )?;
                    indices.pop();
                }
                Ok(())
            }

            let mut indices = Vec::with_capacity(dimensions.len());
            walk_array(
                registry,
                *element,
                dimensions,
                &lengths,
                &strides,
                &reference,
                offset_bytes,
                0,
                &mut indices,
                bit_offset,
                out,
            )
        }
        Type::Struct { fields, .. } => {
            let mut current_offset = offset_bytes;
            for field in fields {
                let mut field_offset = current_offset;
                let mut field_bit_offset = bit_offset;
                if let Some(address) = &field.address {
                    match parse_field_address(address)? {
                        FieldAddress::Relative {
                            offset_bytes: rel_offset,
                            bit_offset: rel_bits,
                        } => {
                            field_offset =
                                offset_bytes.checked_add(rel_offset).ok_or_else(|| {
                                    CompileError::new("struct offset overflow for I/O binding")
                                })?;
                            field_bit_offset = field_bit_offset.saturating_add(rel_bits);
                        }
                        FieldAddress::Absolute(_) => {
                            return Err(CompileError::new(
                                "absolute direct address not allowed for structured fields with a base address",
                            ));
                        }
                    }
                }

                let mut field_ref = reference.clone();
                field_ref
                    .path
                    .push(crate::value::RefSegment::Field(field.name.clone()));
                collect_io_bindings(
                    registry,
                    field.type_id,
                    field_ref,
                    field_offset,
                    field_bit_offset,
                    out,
                )?;
                let field_size = type_size_bytes(field.type_id, registry)?;
                let field_end = field_offset
                    .checked_add(field_size)
                    .ok_or_else(|| CompileError::new("struct offset overflow for I/O binding"))?;
                if field.address.is_some() {
                    current_offset = current_offset.max(field_end);
                } else {
                    current_offset = field_end;
                }
            }
            Ok(())
        }
        Type::Union { variants, .. } => {
            for variant in variants {
                let mut variant_offset = offset_bytes;
                let mut variant_bit_offset = bit_offset;
                if let Some(address) = &variant.address {
                    match parse_field_address(address)? {
                        FieldAddress::Relative {
                            offset_bytes: rel_offset,
                            bit_offset: rel_bits,
                        } => {
                            variant_offset =
                                offset_bytes.checked_add(rel_offset).ok_or_else(|| {
                                    CompileError::new("union offset overflow for I/O binding")
                                })?;
                            variant_bit_offset = variant_bit_offset.saturating_add(rel_bits);
                        }
                        FieldAddress::Absolute(_) => {
                            return Err(CompileError::new(
                                "absolute direct address not allowed for union fields with a base address",
                            ));
                        }
                    }
                }
                let mut variant_ref = reference.clone();
                variant_ref
                    .path
                    .push(crate::value::RefSegment::Field(variant.name.clone()));
                collect_io_bindings(
                    registry,
                    variant.type_id,
                    variant_ref,
                    variant_offset,
                    variant_bit_offset,
                    out,
                )?;
            }
            Ok(())
        }
        Type::String { .. } | Type::WString { .. } => Err(CompileError::new(
            "AT binding for STRING types is not supported",
        )),
        Type::FunctionBlock { .. }
        | Type::Class { .. }
        | Type::Interface { .. }
        | Type::Pointer { .. }
        | Type::Reference { .. } => Err(CompileError::new(
            "AT binding for this type is not supported",
        )),
        _ => {
            let size = io_size_for_type(type_id, registry)?;
            if bit_offset > 0 && !matches!(size, crate::io::IoSize::Bit) {
                return Err(CompileError::new(
                    "bit offset only allowed for BOOL direct bindings",
                ));
            }
            let value_type = leaf_value_type(type_id, registry)?;
            out.push(IoLeafBinding {
                reference,
                offset_bytes,
                bit_offset,
                size,
                value_type,
            });
            Ok(())
        }
    }
}

pub(super) fn collect_direct_field_bindings(
    registry: &TypeRegistry,
    reference: &crate::value::ValueRef,
    type_id: TypeId,
    name: &SmolStr,
    wildcards: &mut Vec<WildcardRequirement>,
    out: &mut Vec<InstanceBinding>,
) -> Result<(), CompileError> {
    let ty = registry
        .get(type_id)
        .ok_or_else(|| CompileError::new("unknown type for direct field binding"))?;
    match ty {
        Type::Alias { target, .. } => {
            collect_direct_field_bindings(registry, reference, *target, name, wildcards, out)
        }
        Type::Subrange { base, .. } => {
            collect_direct_field_bindings(registry, reference, *base, name, wildcards, out)
        }
        Type::Enum { base, .. } => {
            collect_direct_field_bindings(registry, reference, *base, name, wildcards, out)
        }
        Type::Struct { fields, .. } => {
            for field in fields {
                let mut field_ref = reference.clone();
                field_ref
                    .path
                    .push(crate::value::RefSegment::Field(field.name.clone()));
                let field_name = join_instance_path(name, &field.name);
                if let Some(address) = &field.address {
                    match parse_field_address(address)? {
                        FieldAddress::Absolute(address) => {
                            if address.wildcard {
                                wildcards.push(WildcardRequirement {
                                    name: field_name,
                                    reference: field_ref,
                                    area: address.area,
                                });
                            } else {
                                out.push(InstanceBinding {
                                    reference: field_ref,
                                    type_id: field.type_id,
                                    address,
                                    display_name: field_name,
                                });
                            }
                        }
                        FieldAddress::Relative { .. } => {
                            continue;
                        }
                    }
                    continue;
                }
                collect_direct_field_bindings(
                    registry,
                    &field_ref,
                    field.type_id,
                    &field_name,
                    wildcards,
                    out,
                )?;
            }
            Ok(())
        }
        Type::Union { variants, .. } => {
            for variant in variants {
                let mut variant_ref = reference.clone();
                variant_ref
                    .path
                    .push(crate::value::RefSegment::Field(variant.name.clone()));
                let variant_name = join_instance_path(name, &variant.name);
                if let Some(address) = &variant.address {
                    match parse_field_address(address)? {
                        FieldAddress::Absolute(address) => {
                            if address.wildcard {
                                wildcards.push(WildcardRequirement {
                                    name: variant_name,
                                    reference: variant_ref,
                                    area: address.area,
                                });
                            } else {
                                out.push(InstanceBinding {
                                    reference: variant_ref,
                                    type_id: variant.type_id,
                                    address,
                                    display_name: variant_name,
                                });
                            }
                        }
                        FieldAddress::Relative { .. } => {
                            continue;
                        }
                    }
                    continue;
                }
                collect_direct_field_bindings(
                    registry,
                    &variant_ref,
                    variant.type_id,
                    &variant_name,
                    wildcards,
                    out,
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn type_size_bytes(type_id: TypeId, registry: &TypeRegistry) -> Result<u64, CompileError> {
    crate::value::size_of_type(type_id, registry)
        .map_err(|err| CompileError::new(format!("unsupported size for I/O binding: {err:?}")))
}

fn io_size_for_type(
    type_id: TypeId,
    registry: &TypeRegistry,
) -> Result<crate::io::IoSize, CompileError> {
    let ty = registry
        .get(type_id)
        .ok_or_else(|| CompileError::new("unknown type for I/O binding"))?;
    match ty {
        Type::Alias { target, .. } => io_size_for_type(*target, registry),
        Type::Subrange { base, .. } => io_size_for_type(*base, registry),
        Type::Enum { base, .. } => io_size_for_type(*base, registry),
        Type::Bool => Ok(crate::io::IoSize::Bit),
        Type::SInt | Type::USInt | Type::Byte | Type::Char => Ok(crate::io::IoSize::Byte),
        Type::Int | Type::UInt | Type::Word | Type::WChar => Ok(crate::io::IoSize::Word),
        Type::DInt
        | Type::UDInt
        | Type::DWord
        | Type::Real
        | Type::Time
        | Type::Date
        | Type::Tod
        | Type::Dt => Ok(crate::io::IoSize::DWord),
        Type::LInt
        | Type::ULInt
        | Type::LWord
        | Type::LReal
        | Type::LTime
        | Type::LDate
        | Type::LTod
        | Type::Ldt => Ok(crate::io::IoSize::LWord),
        _ => Err(CompileError::new("unsupported type for I/O binding")),
    }
}

fn leaf_value_type(type_id: TypeId, registry: &TypeRegistry) -> Result<TypeId, CompileError> {
    let ty = registry
        .get(type_id)
        .ok_or_else(|| CompileError::new("unknown type for I/O binding"))?;
    match ty {
        Type::Alias { target, .. } => leaf_value_type(*target, registry),
        Type::Subrange { base, .. } => Ok(*base),
        Type::Enum { base, .. } => Ok(*base),
        _ => Ok(type_id),
    }
}

fn offset_address(
    base: &IoAddress,
    offset_bytes: u64,
    size: crate::io::IoSize,
    bit_offset: u8,
) -> Result<IoAddress, CompileError> {
    let mut address = base.clone();
    address.size = size;
    address.wildcard = false;

    let offset_bytes_u32 = u32::try_from(offset_bytes)
        .map_err(|_| CompileError::new("I/O address offset overflow"))?;

    if matches!(size, crate::io::IoSize::Bit) {
        let total_bits = u64::from(base.bit) + offset_bytes * 8 + u64::from(bit_offset);
        let add_bytes = total_bits / 8;
        let bit = (total_bits % 8) as u8;
        let add_bytes_u32 = u32::try_from(add_bytes)
            .map_err(|_| CompileError::new("I/O address offset overflow"))?;
        address.bit = bit;
        if address.path.len() > 1 {
            let mut path = address.path.clone();
            let last = path
                .last_mut()
                .ok_or_else(|| CompileError::new("invalid I/O address path"))?;
            *last = last
                .checked_add(add_bytes_u32)
                .ok_or_else(|| CompileError::new("I/O address offset overflow"))?;
            address.path = path;
            address.byte = address.path[0];
        } else {
            address.byte = base
                .byte
                .checked_add(add_bytes_u32)
                .ok_or_else(|| CompileError::new("I/O address offset overflow"))?;
            address.path = vec![address.byte];
        }
        return Ok(address);
    }

    address.bit = 0;
    if address.path.len() > 1 {
        let mut path = address.path.clone();
        let last = path
            .last_mut()
            .ok_or_else(|| CompileError::new("invalid I/O address path"))?;
        *last = last
            .checked_add(offset_bytes_u32)
            .ok_or_else(|| CompileError::new("I/O address offset overflow"))?;
        address.path = path;
        address.byte = address.path[0];
    } else {
        address.byte = base
            .byte
            .checked_add(offset_bytes_u32)
            .ok_or_else(|| CompileError::new("I/O address offset overflow"))?;
        address.path = vec![address.byte];
    }
    Ok(address)
}
