//! Bytecode container format types.

#![allow(missing_docs)]

use smol_str::SmolStr;
use thiserror::Error;

use crate::task::TaskConfig;

/// Bytecode format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytecodeVersion {
    pub major: u16,
    pub minor: u16,
}

impl BytecodeVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

/// Supported major bytecode version.
pub const SUPPORTED_MAJOR_VERSION: u16 = 1;
pub const SUPPORTED_MINOR_VERSION: u16 = 1;

pub(crate) const MAGIC: [u8; 4] = *b"STBC";
pub(crate) const HEADER_SIZE: u16 = 24;
pub(crate) const SECTION_ENTRY_SIZE: usize = 12;
pub(crate) const HEADER_FLAG_CRC32: u32 = 0x0001;

/// Process image sizing derived from bytecode metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessImageConfig {
    pub inputs: usize,
    pub outputs: usize,
    pub memory: usize,
}

/// Resource metadata captured in a bytecode module.
#[derive(Debug, Clone)]
pub struct ResourceMetadata {
    pub name: SmolStr,
    pub process_image: ProcessImageConfig,
    pub tasks: Vec<TaskConfig>,
}

/// Bytecode metadata for a configuration.
#[derive(Debug, Clone)]
pub struct BytecodeMetadata {
    pub version: BytecodeVersion,
    pub resources: Vec<ResourceMetadata>,
}

impl BytecodeMetadata {
    /// Lookup a resource by name.
    #[must_use]
    pub fn resource(&self, name: &str) -> Option<&ResourceMetadata> {
        self.resources
            .iter()
            .find(|resource| resource.name.eq_ignore_ascii_case(name))
    }

    /// Return the first resource, if any.
    #[must_use]
    pub fn primary_resource(&self) -> Option<&ResourceMetadata> {
        self.resources.first()
    }
}

/// Bytecode decoder errors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BytecodeError {
    #[error("invalid bytecode magic")]
    InvalidMagic,
    #[error("unsupported bytecode version {major}.{minor}")]
    UnsupportedVersion { major: u16, minor: u16 },
    #[error("invalid bytecode header: {0}")]
    InvalidHeader(SmolStr),
    #[error("invalid bytecode checksum (expected {expected:#010x}, got {actual:#010x})")]
    InvalidChecksum { expected: u32, actual: u32 },
    #[error("invalid section table: {0}")]
    InvalidSectionTable(SmolStr),
    #[error("section out of bounds")]
    SectionOutOfBounds,
    #[error("section overlap")]
    SectionOverlap,
    #[error("section alignment error")]
    SectionAlignment,
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("invalid section data: {0}")]
    InvalidSection(SmolStr),
    #[error("missing required section: {0}")]
    MissingSection(SmolStr),
    #[error("invalid opcode 0x{0:02X}")]
    InvalidOpcode(u8),
    #[error("invalid jump target {0}")]
    InvalidJumpTarget(i32),
    #[error("invalid POU id {0}")]
    InvalidPouId(u32),
    #[error("invalid index {index} for {kind}")]
    InvalidIndex { kind: SmolStr, index: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionId {
    StringTable = 0x0001,
    TypeTable = 0x0002,
    ConstPool = 0x0003,
    RefTable = 0x0004,
    PouIndex = 0x0005,
    PouBodies = 0x0006,
    ResourceMeta = 0x0007,
    IoMap = 0x0008,
    DebugMap = 0x0009,
    DebugStringTable = 0x000A,
    VarMeta = 0x000B,
    RetainInit = 0x000C,
}

impl SectionId {
    #[must_use]
    pub fn from_raw(id: u16) -> Option<Self> {
        match id {
            0x0001 => Some(Self::StringTable),
            0x0002 => Some(Self::TypeTable),
            0x0003 => Some(Self::ConstPool),
            0x0004 => Some(Self::RefTable),
            0x0005 => Some(Self::PouIndex),
            0x0006 => Some(Self::PouBodies),
            0x0007 => Some(Self::ResourceMeta),
            0x0008 => Some(Self::IoMap),
            0x0009 => Some(Self::DebugMap),
            0x000A => Some(Self::DebugStringTable),
            0x000B => Some(Self::VarMeta),
            0x000C => Some(Self::RetainInit),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_raw(self) -> u16 {
        self as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionEntry {
    pub id: u16,
    pub flags: u16,
    pub offset: u32,
    pub length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: u16,
    pub flags: u16,
    pub data: SectionData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionData {
    StringTable(StringTable),
    DebugStringTable(StringTable),
    TypeTable(TypeTable),
    ConstPool(ConstPool),
    RefTable(RefTable),
    PouIndex(PouIndex),
    PouBodies(Vec<u8>),
    ResourceMeta(ResourceMeta),
    IoMap(IoMap),
    DebugMap(DebugMap),
    VarMeta(VarMeta),
    RetainInit(RetainInit),
    Raw(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StringTable {
    pub entries: Vec<SmolStr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TypeTable {
    pub offsets: Vec<u32>,
    pub entries: Vec<TypeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeEntry {
    pub kind: TypeKind,
    pub name_idx: Option<u32>,
    pub data: TypeData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Primitive = 0,
    Array = 1,
    Struct = 2,
    Enum = 3,
    Alias = 4,
    Subrange = 5,
    Reference = 6,
    Union = 7,
    FunctionBlock = 8,
    Class = 9,
    Interface = 10,
}

impl TypeKind {
    pub(crate) fn from_raw(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Primitive),
            1 => Some(Self::Array),
            2 => Some(Self::Struct),
            3 => Some(Self::Enum),
            4 => Some(Self::Alias),
            5 => Some(Self::Subrange),
            6 => Some(Self::Reference),
            7 => Some(Self::Union),
            8 => Some(Self::FunctionBlock),
            9 => Some(Self::Class),
            10 => Some(Self::Interface),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeData {
    Primitive {
        prim_id: u16,
        max_length: u16,
    },
    Array {
        elem_type_id: u32,
        dims: Vec<(i64, i64)>,
    },
    Struct {
        fields: Vec<Field>,
    },
    Enum {
        base_type_id: u32,
        variants: Vec<EnumVariant>,
    },
    Alias {
        target_type_id: u32,
    },
    Subrange {
        base_type_id: u32,
        lower: i64,
        upper: i64,
    },
    Reference {
        target_type_id: u32,
    },
    Union {
        fields: Vec<Field>,
    },
    Pou {
        pou_id: u32,
    },
    Interface {
        methods: Vec<InterfaceMethod>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name_idx: u32,
    pub type_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name_idx: u32,
    pub value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceMethod {
    pub name_idx: u32,
    pub slot: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConstPool {
    pub entries: Vec<ConstEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstEntry {
    pub type_id: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RefTable {
    pub entries: Vec<RefEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEntry {
    pub location: RefLocation,
    pub owner_id: u32,
    pub offset: u32,
    pub segments: Vec<RefSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefLocation {
    Global = 0,
    Local = 1,
    Instance = 2,
    Io = 3,
    Retain = 4,
}

impl RefLocation {
    pub(crate) fn from_raw(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Global),
            1 => Some(Self::Local),
            2 => Some(Self::Instance),
            3 => Some(Self::Io),
            4 => Some(Self::Retain),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefSegment {
    Index(Vec<i64>),
    Field { name_idx: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PouIndex {
    pub entries: Vec<PouEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PouEntry {
    pub id: u32,
    pub name_idx: u32,
    pub kind: PouKind,
    pub code_offset: u32,
    pub code_length: u32,
    pub local_ref_start: u32,
    pub local_ref_count: u32,
    pub return_type_id: Option<u32>,
    pub owner_pou_id: Option<u32>,
    pub params: Vec<ParamEntry>,
    pub class_meta: Option<PouClassMeta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PouKind {
    Program = 0,
    FunctionBlock = 1,
    Function = 2,
    Class = 3,
    Method = 4,
}

impl PouKind {
    pub(crate) fn from_raw(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Program),
            1 => Some(Self::FunctionBlock),
            2 => Some(Self::Function),
            3 => Some(Self::Class),
            4 => Some(Self::Method),
            _ => None,
        }
    }

    pub(crate) fn is_class_like(self) -> bool {
        matches!(self, Self::FunctionBlock | Self::Class)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamEntry {
    pub name_idx: u32,
    pub type_id: u32,
    pub direction: u8,
    pub default_const_idx: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VarMeta {
    pub entries: Vec<VarMetaEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarMetaEntry {
    pub name_idx: u32,
    pub type_id: u32,
    pub ref_idx: u32,
    pub retain: u8,
    pub init_const_idx: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetainInit {
    pub entries: Vec<RetainInitEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainInitEntry {
    pub ref_idx: u32,
    pub const_idx: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PouClassMeta {
    pub parent_pou_id: Option<u32>,
    pub interfaces: Vec<InterfaceImpl>,
    pub methods: Vec<MethodEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodEntry {
    pub name_idx: u32,
    pub pou_id: u32,
    pub vtable_slot: u32,
    pub access: u8,
    pub flags: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceImpl {
    pub interface_type_id: u32,
    pub vtable_slots: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResourceMeta {
    pub resources: Vec<ResourceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEntry {
    pub name_idx: u32,
    pub inputs_size: u32,
    pub outputs_size: u32,
    pub memory_size: u32,
    pub tasks: Vec<TaskEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEntry {
    pub name_idx: u32,
    pub priority: u32,
    pub interval_nanos: i64,
    pub single_name_idx: Option<u32>,
    pub program_name_idx: Vec<u32>,
    pub fb_ref_idx: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IoMap {
    pub bindings: Vec<IoBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoBinding {
    pub address_str_idx: u32,
    pub ref_idx: u32,
    pub type_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DebugMap {
    pub entries: Vec<DebugEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugEntry {
    pub pou_id: u32,
    pub code_offset: u32,
    pub file_idx: u32,
    pub line: u32,
    pub column: u32,
    pub kind: u8,
}

/// Decoded bytecode module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeModule {
    pub version: BytecodeVersion,
    pub flags: u32,
    pub sections: Vec<Section>,
}

impl BytecodeModule {
    #[must_use]
    pub fn new(version: BytecodeVersion) -> Self {
        let flags = if version.minor >= 1 {
            HEADER_FLAG_CRC32
        } else {
            0
        };
        Self {
            version,
            flags,
            sections: Vec::new(),
        }
    }

    #[must_use]
    pub fn section(&self, id: SectionId) -> Option<&SectionData> {
        self.sections
            .iter()
            .find(|section| section.id == id.as_raw())
            .map(|section| &section.data)
    }

    #[must_use]
    pub fn section_mut(&mut self, id: SectionId) -> Option<&mut SectionData> {
        self.sections
            .iter_mut()
            .find(|section| section.id == id.as_raw())
            .map(|section| &mut section.data)
    }
}
