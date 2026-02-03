//! Bytecode decoding.

#![allow(missing_docs)]

use smol_str::SmolStr;

use super::reader::BytecodeReader;
use super::util::align4;
use super::{
    BytecodeError, BytecodeModule, BytecodeVersion, ConstEntry, ConstPool, DebugEntry, DebugMap,
    EnumVariant, Field, InterfaceImpl, InterfaceMethod, IoBinding, IoMap, MethodEntry,
    PouClassMeta, PouEntry, PouIndex, PouKind, RefEntry, RefLocation, RefSegment, RefTable,
    ResourceEntry, ResourceMeta, RetainInit, RetainInitEntry, Section, SectionData, SectionEntry,
    SectionId, StringTable, TypeData, TypeEntry, TypeKind, TypeTable, VarMeta, VarMetaEntry,
    HEADER_FLAG_CRC32, HEADER_SIZE, MAGIC, SECTION_ENTRY_SIZE, SUPPORTED_MAJOR_VERSION,
};

impl BytecodeModule {
    pub fn decode(bytes: &[u8]) -> Result<Self, BytecodeError> {
        let mut reader = BytecodeReader::new(bytes);
        let magic = reader.read_bytes(4)?;
        if magic != MAGIC {
            return Err(BytecodeError::InvalidMagic);
        }
        let major = reader.read_u16()?;
        let minor = reader.read_u16()?;
        let flags = reader.read_u32()?;
        let header_size = reader.read_u16()?;
        let section_count = reader.read_u16()? as usize;
        let section_table_off = reader.read_u32()? as usize;
        let checksum = reader.read_u32()?;

        if header_size < HEADER_SIZE {
            return Err(BytecodeError::InvalidHeader("header size too small".into()));
        }
        if section_table_off < HEADER_SIZE as usize {
            return Err(BytecodeError::InvalidHeader(
                "section table before header".into(),
            ));
        }
        if section_table_off % 4 != 0 {
            return Err(BytecodeError::SectionAlignment);
        }
        let table_len = section_count
            .checked_mul(SECTION_ENTRY_SIZE)
            .ok_or_else(|| BytecodeError::InvalidSectionTable("section table overflow".into()))?;
        let table_end = section_table_off
            .checked_add(table_len)
            .ok_or_else(|| BytecodeError::InvalidSectionTable("section table overflow".into()))?;
        if table_end > bytes.len() {
            return Err(BytecodeError::InvalidSectionTable(
                "section table out of bounds".into(),
            ));
        }

        if flags & HEADER_FLAG_CRC32 != 0 {
            let actual = crc32fast::hash(&bytes[section_table_off..]);
            if actual != checksum {
                return Err(BytecodeError::InvalidChecksum {
                    expected: checksum,
                    actual,
                });
            }
        }

        if major != SUPPORTED_MAJOR_VERSION {
            return Err(BytecodeError::UnsupportedVersion { major, minor });
        }

        let mut entries = Vec::with_capacity(section_count);
        let mut table_reader = BytecodeReader::new(&bytes[section_table_off..table_end]);
        for _ in 0..section_count {
            let id = table_reader.read_u16()?;
            let flags = table_reader.read_u16()?;
            let offset = table_reader.read_u32()?;
            let length = table_reader.read_u32()?;
            entries.push(SectionEntry {
                id,
                flags,
                offset,
                length,
            });
        }

        validate_section_entries(bytes.len(), &entries)?;

        let mut sections = Vec::new();
        for entry in entries {
            let start = entry.offset as usize;
            let end = start + entry.length as usize;
            let payload = &bytes[start..end];
            let data = decode_section_data(BytecodeVersion { major, minor }, entry.id, payload)?;
            sections.push(Section {
                id: entry.id,
                flags: entry.flags,
                data,
            });
        }

        Ok(Self {
            version: BytecodeVersion { major, minor },
            flags,
            sections,
        })
    }
}

fn decode_section_data(
    version: BytecodeVersion,
    id: u16,
    payload: &[u8],
) -> Result<SectionData, BytecodeError> {
    let Some(kind) = SectionId::from_raw(id) else {
        return Ok(SectionData::Raw(payload.to_vec()));
    };
    let mut reader = BytecodeReader::new(payload);
    let data = match kind {
        SectionId::StringTable | SectionId::DebugStringTable => {
            let table = decode_string_table(version, &mut reader)?;
            match kind {
                SectionId::StringTable => SectionData::StringTable(table),
                SectionId::DebugStringTable => SectionData::DebugStringTable(table),
                _ => unreachable!("string table branch"),
            }
        }
        SectionId::TypeTable => SectionData::TypeTable(decode_type_table(version, payload)?),
        SectionId::ConstPool => {
            let count = reader.read_u32()? as usize;
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                let type_id = reader.read_u32()?;
                let len = reader.read_u32()? as usize;
                let payload = reader.read_bytes(len)?.to_vec();
                entries.push(ConstEntry { type_id, payload });
            }
            SectionData::ConstPool(ConstPool { entries })
        }
        SectionId::RefTable => {
            let count = reader.read_u32()? as usize;
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                let location = reader.read_u8()?;
                let _flags = reader.read_u8()?;
                let _reserved = reader.read_u16()?;
                let owner_id = reader.read_u32()?;
                let offset = reader.read_u32()?;
                let segment_count = reader.read_u32()? as usize;
                let location = RefLocation::from_raw(location)
                    .ok_or_else(|| BytecodeError::InvalidSection("invalid ref location".into()))?;
                let mut segments = Vec::with_capacity(segment_count);
                for _ in 0..segment_count {
                    let kind = reader.read_u8()?;
                    let _reserved = reader.read_bytes(3)?;
                    match kind {
                        0 => {
                            let count = reader.read_u32()? as usize;
                            let mut indices = Vec::with_capacity(count);
                            for _ in 0..count {
                                indices.push(reader.read_i64()?);
                            }
                            segments.push(RefSegment::Index(indices));
                        }
                        1 => {
                            let name_idx = reader.read_u32()?;
                            segments.push(RefSegment::Field { name_idx });
                        }
                        _ => {
                            return Err(BytecodeError::InvalidSection("invalid ref segment".into()))
                        }
                    }
                }
                entries.push(RefEntry {
                    location,
                    owner_id,
                    offset,
                    segments,
                });
            }
            SectionData::RefTable(RefTable { entries })
        }
        SectionId::PouIndex => {
            let count = reader.read_u32()? as usize;
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                let id = reader.read_u32()?;
                let name_idx = reader.read_u32()?;
                let kind = reader.read_u8()?;
                let _flags = reader.read_u8()?;
                let _reserved = reader.read_u16()?;
                let code_offset = reader.read_u32()?;
                let code_length = reader.read_u32()?;
                let local_ref_start = reader.read_u32()?;
                let local_ref_count = reader.read_u32()?;
                let return_type_id = reader.read_u32()?;
                let owner_pou_id = reader.read_u32()?;
                let param_count = reader.read_u32()? as usize;
                let kind = PouKind::from_raw(kind)
                    .ok_or_else(|| BytecodeError::InvalidSection("invalid pou kind".into()))?;
                let return_type_id = if return_type_id == u32::MAX {
                    None
                } else {
                    Some(return_type_id)
                };
                let owner_pou_id = if owner_pou_id == u32::MAX {
                    None
                } else {
                    Some(owner_pou_id)
                };
                let mut params = Vec::with_capacity(param_count);
                for _ in 0..param_count {
                    let name_idx = reader.read_u32()?;
                    let type_id = reader.read_u32()?;
                    let direction = reader.read_u8()?;
                    let _flags = reader.read_u8()?;
                    let _reserved = reader.read_u16()?;
                    let default_const_idx = if version.minor >= 1 {
                        let idx = reader.read_u32()?;
                        if idx == u32::MAX {
                            None
                        } else {
                            Some(idx)
                        }
                    } else {
                        None
                    };
                    params.push(super::ParamEntry {
                        name_idx,
                        type_id,
                        direction,
                        default_const_idx,
                    });
                }
                let class_meta = if kind.is_class_like() {
                    let parent_pou_id = reader.read_u32()?;
                    let parent_pou_id = if parent_pou_id == u32::MAX {
                        None
                    } else {
                        Some(parent_pou_id)
                    };
                    let interface_count = reader.read_u32()? as usize;
                    let mut interfaces = Vec::with_capacity(interface_count);
                    for _ in 0..interface_count {
                        let interface_type_id = reader.read_u32()?;
                        let method_count = reader.read_u32()? as usize;
                        let mut vtable_slots = Vec::with_capacity(method_count);
                        for _ in 0..method_count {
                            vtable_slots.push(reader.read_u32()?);
                        }
                        interfaces.push(InterfaceImpl {
                            interface_type_id,
                            vtable_slots,
                        });
                    }
                    let method_count = reader.read_u32()? as usize;
                    let mut methods = Vec::with_capacity(method_count);
                    for _ in 0..method_count {
                        let name_idx = reader.read_u32()?;
                        let pou_id = reader.read_u32()?;
                        let vtable_slot = reader.read_u32()?;
                        let access = reader.read_u8()?;
                        let flags = reader.read_u8()?;
                        let _reserved = reader.read_u16()?;
                        methods.push(MethodEntry {
                            name_idx,
                            pou_id,
                            vtable_slot,
                            access,
                            flags,
                        });
                    }
                    Some(PouClassMeta {
                        parent_pou_id,
                        interfaces,
                        methods,
                    })
                } else {
                    None
                };
                entries.push(PouEntry {
                    id,
                    name_idx,
                    kind,
                    code_offset,
                    code_length,
                    local_ref_start,
                    local_ref_count,
                    return_type_id,
                    owner_pou_id,
                    params,
                    class_meta,
                });
            }
            SectionData::PouIndex(PouIndex { entries })
        }
        SectionId::PouBodies => SectionData::PouBodies(payload.to_vec()),
        SectionId::ResourceMeta => {
            let resource_count = reader.read_u32()? as usize;
            let mut resources = Vec::with_capacity(resource_count);
            for _ in 0..resource_count {
                let name_idx = reader.read_u32()?;
                let inputs_size = reader.read_u32()?;
                let outputs_size = reader.read_u32()?;
                let memory_size = reader.read_u32()?;
                let task_count = reader.read_u32()? as usize;
                let mut tasks = Vec::with_capacity(task_count);
                for _ in 0..task_count {
                    let name_idx = reader.read_u32()?;
                    let priority = reader.read_u32()?;
                    let interval_nanos = reader.read_i64()?;
                    let single_name_idx = reader.read_u32()?;
                    let single_name_idx = if single_name_idx == u32::MAX {
                        None
                    } else {
                        Some(single_name_idx)
                    };
                    let program_count = reader.read_u32()? as usize;
                    let mut program_name_idx = Vec::with_capacity(program_count);
                    for _ in 0..program_count {
                        program_name_idx.push(reader.read_u32()?);
                    }
                    let fb_ref_count = reader.read_u32()? as usize;
                    let mut fb_ref_idx = Vec::with_capacity(fb_ref_count);
                    for _ in 0..fb_ref_count {
                        fb_ref_idx.push(reader.read_u32()?);
                    }
                    tasks.push(super::TaskEntry {
                        name_idx,
                        priority,
                        interval_nanos,
                        single_name_idx,
                        program_name_idx,
                        fb_ref_idx,
                    });
                }
                resources.push(ResourceEntry {
                    name_idx,
                    inputs_size,
                    outputs_size,
                    memory_size,
                    tasks,
                });
            }
            SectionData::ResourceMeta(ResourceMeta { resources })
        }
        SectionId::IoMap => {
            let binding_count = reader.read_u32()? as usize;
            let mut bindings = Vec::with_capacity(binding_count);
            for _ in 0..binding_count {
                let address_str_idx = reader.read_u32()?;
                let ref_idx = reader.read_u32()?;
                let type_id = reader.read_u32()?;
                let type_id = if type_id == u32::MAX {
                    None
                } else {
                    Some(type_id)
                };
                bindings.push(IoBinding {
                    address_str_idx,
                    ref_idx,
                    type_id,
                });
            }
            SectionData::IoMap(IoMap { bindings })
        }
        SectionId::DebugMap => {
            let entry_count = reader.read_u32()? as usize;
            let mut entries = Vec::with_capacity(entry_count);
            for _ in 0..entry_count {
                let pou_id = reader.read_u32()?;
                let code_offset = reader.read_u32()?;
                let file_idx = reader.read_u32()?;
                let line = reader.read_u32()?;
                let column = reader.read_u32()?;
                let kind = reader.read_u8()?;
                let _reserved = reader.read_bytes(3)?;
                entries.push(DebugEntry {
                    pou_id,
                    code_offset,
                    file_idx,
                    line,
                    column,
                    kind,
                });
            }
            SectionData::DebugMap(DebugMap { entries })
        }
        SectionId::VarMeta => {
            let entry_count = reader.read_u32()? as usize;
            let mut entries = Vec::with_capacity(entry_count);
            for _ in 0..entry_count {
                let name_idx = reader.read_u32()?;
                let type_id = reader.read_u32()?;
                let ref_idx = reader.read_u32()?;
                let retain = reader.read_u8()?;
                let _flags = reader.read_u8()?;
                let _reserved = reader.read_u16()?;
                let init_const_idx = reader.read_u32()?;
                let init_const_idx = if init_const_idx == u32::MAX {
                    None
                } else {
                    Some(init_const_idx)
                };
                entries.push(VarMetaEntry {
                    name_idx,
                    type_id,
                    ref_idx,
                    retain,
                    init_const_idx,
                });
            }
            SectionData::VarMeta(VarMeta { entries })
        }
        SectionId::RetainInit => {
            let entry_count = reader.read_u32()? as usize;
            let mut entries = Vec::with_capacity(entry_count);
            for _ in 0..entry_count {
                let ref_idx = reader.read_u32()?;
                let const_idx = reader.read_u32()?;
                entries.push(RetainInitEntry { ref_idx, const_idx });
            }
            SectionData::RetainInit(RetainInit { entries })
        }
    };
    Ok(data)
}

fn decode_string_table(
    version: BytecodeVersion,
    reader: &mut BytecodeReader<'_>,
) -> Result<StringTable, BytecodeError> {
    let count = reader.read_u32()? as usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let len = reader.read_u32()? as usize;
        let bytes = reader.read_bytes(len)?;
        let string = std::str::from_utf8(bytes)
            .map_err(|_| BytecodeError::InvalidSection("invalid utf-8".into()))?;
        entries.push(SmolStr::new(string));
        if version.minor >= 1 {
            let entry_len = 4usize + len;
            let padded = align4(entry_len);
            let padding = padded.saturating_sub(entry_len);
            if padding > 0 {
                reader.read_bytes(padding)?;
            }
        }
    }
    Ok(StringTable { entries })
}

fn decode_type_table(version: BytecodeVersion, payload: &[u8]) -> Result<TypeTable, BytecodeError> {
    let mut reader = BytecodeReader::new(payload);
    let count = reader.read_u32()? as usize;
    if version.minor >= 1 {
        let mut offsets = Vec::with_capacity(count);
        for _ in 0..count {
            offsets.push(reader.read_u32()?);
        }
        let base = reader.pos();
        let mut entries = Vec::with_capacity(count);
        for (idx, offset) in offsets.iter().enumerate() {
            let offset = *offset as usize;
            let next = if idx + 1 < offsets.len() {
                offsets[idx + 1] as usize
            } else {
                payload.len()
            };
            if offset < base || offset > payload.len() || next > payload.len() || next < offset {
                return Err(BytecodeError::InvalidSection(
                    "type table offset out of bounds".into(),
                ));
            }
            if idx > 0 && offset < offsets[idx - 1] as usize {
                return Err(BytecodeError::InvalidSection(
                    "type table offsets not sorted".into(),
                ));
            }
            let mut entry_reader = BytecodeReader::new(&payload[offset..next]);
            let entry = decode_type_entry(&mut entry_reader)?;
            if entry_reader.remaining() != 0 {
                return Err(BytecodeError::InvalidSection(
                    "type entry length mismatch".into(),
                ));
            }
            entries.push(entry);
        }
        Ok(TypeTable { offsets, entries })
    } else {
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(decode_type_entry(&mut reader)?);
        }
        Ok(TypeTable {
            offsets: Vec::new(),
            entries,
        })
    }
}

fn decode_type_entry(reader: &mut BytecodeReader<'_>) -> Result<TypeEntry, BytecodeError> {
    let kind = reader.read_u8()?;
    let _flags = reader.read_u8()?;
    let _reserved = reader.read_u16()?;
    let name_idx = reader.read_u32()?;
    let name_idx = if name_idx == u32::MAX {
        None
    } else {
        Some(name_idx)
    };
    let kind = TypeKind::from_raw(kind)
        .ok_or_else(|| BytecodeError::InvalidSection("invalid type kind".into()))?;
    let data = match kind {
        TypeKind::Primitive => {
            let prim_id = reader.read_u16()?;
            let max_length = reader.read_u16()?;
            TypeData::Primitive {
                prim_id,
                max_length,
            }
        }
        TypeKind::Array => {
            let elem_type_id = reader.read_u32()?;
            let dim_count = reader.read_u32()? as usize;
            let mut dims = Vec::with_capacity(dim_count);
            for _ in 0..dim_count {
                let lower = reader.read_i64()?;
                let upper = reader.read_i64()?;
                dims.push((lower, upper));
            }
            TypeData::Array { elem_type_id, dims }
        }
        TypeKind::Struct => {
            let field_count = reader.read_u32()? as usize;
            let mut fields = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                let name_idx = reader.read_u32()?;
                let type_id = reader.read_u32()?;
                fields.push(Field { name_idx, type_id });
            }
            TypeData::Struct { fields }
        }
        TypeKind::Enum => {
            let base_type_id = reader.read_u32()?;
            let variant_count = reader.read_u32()? as usize;
            let mut variants = Vec::with_capacity(variant_count);
            for _ in 0..variant_count {
                let name_idx = reader.read_u32()?;
                let value = reader.read_i64()?;
                variants.push(EnumVariant { name_idx, value });
            }
            TypeData::Enum {
                base_type_id,
                variants,
            }
        }
        TypeKind::Alias => {
            let target_type_id = reader.read_u32()?;
            TypeData::Alias { target_type_id }
        }
        TypeKind::Subrange => {
            let base_type_id = reader.read_u32()?;
            let lower = reader.read_i64()?;
            let upper = reader.read_i64()?;
            TypeData::Subrange {
                base_type_id,
                lower,
                upper,
            }
        }
        TypeKind::Reference => {
            let target_type_id = reader.read_u32()?;
            TypeData::Reference { target_type_id }
        }
        TypeKind::Union => {
            let field_count = reader.read_u32()? as usize;
            let mut fields = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                let name_idx = reader.read_u32()?;
                let type_id = reader.read_u32()?;
                fields.push(Field { name_idx, type_id });
            }
            TypeData::Union { fields }
        }
        TypeKind::FunctionBlock | TypeKind::Class => {
            let pou_id = reader.read_u32()?;
            TypeData::Pou { pou_id }
        }
        TypeKind::Interface => {
            let method_count = reader.read_u32()? as usize;
            let mut methods = Vec::with_capacity(method_count);
            for _ in 0..method_count {
                let name_idx = reader.read_u32()?;
                let slot = reader.read_u32()?;
                methods.push(InterfaceMethod { name_idx, slot });
            }
            TypeData::Interface { methods }
        }
    };
    Ok(TypeEntry {
        kind,
        name_idx,
        data,
    })
}

fn validate_section_entries(
    file_len: usize,
    entries: &[SectionEntry],
) -> Result<(), BytecodeError> {
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|entry| entry.offset);
    let mut last_end = 0usize;
    for entry in sorted {
        if entry.offset % 4 != 0 {
            return Err(BytecodeError::SectionAlignment);
        }
        let start = entry.offset as usize;
        let end = start + entry.length as usize;
        if end > file_len {
            return Err(BytecodeError::SectionOutOfBounds);
        }
        if start < last_end {
            return Err(BytecodeError::SectionOverlap);
        }
        last_end = end;
    }
    Ok(())
}
