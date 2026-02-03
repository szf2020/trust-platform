//! Retain storage support.

#![allow(missing_docs)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use smol_str::SmolStr;

use crate::error::RuntimeError;
use crate::runtime::RetainSnapshot;
use crate::value::{
    ArrayValue, DateTimeValue, DateValue, Duration, EnumValue, LDateTimeValue, LDateValue,
    LTimeOfDayValue, StructValue, TimeOfDayValue, Value,
};
use crate::Runtime;

const RETAIN_MAGIC: &[u8; 4] = b"STRN";
const RETAIN_VERSION: u16 = 1;

/// Retain storage backend.
pub trait RetainStore: Send {
    fn load(&self) -> Result<RetainSnapshot, RuntimeError>;
    fn store(&self, snapshot: &RetainSnapshot) -> Result<(), RuntimeError>;
}

pub struct RetainManager {
    store: Option<Box<dyn RetainStore>>,
    save_interval: Option<Duration>,
    last_save: Duration,
    dirty: bool,
    last_snapshot: Option<RetainSnapshot>,
}

impl Default for RetainManager {
    fn default() -> Self {
        Self {
            store: None,
            save_interval: None,
            last_save: Duration::ZERO,
            dirty: false,
            last_snapshot: None,
        }
    }
}

impl std::fmt::Debug for RetainManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainManager")
            .field("store_configured", &self.store.is_some())
            .field("save_interval", &self.save_interval)
            .field("last_save", &self.last_save)
            .field("dirty", &self.dirty)
            .field("has_snapshot", &self.last_snapshot.is_some())
            .finish()
    }
}

impl RetainManager {
    pub fn configure(
        &mut self,
        store: Option<Box<dyn RetainStore>>,
        save_interval: Option<Duration>,
        now: Duration,
    ) {
        self.store = store;
        self.save_interval = save_interval;
        self.last_save = now;
        self.dirty = false;
        self.last_snapshot = None;
    }

    pub fn set_save_interval(&mut self, interval: Option<Duration>) {
        self.save_interval = interval;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn has_store(&self) -> bool {
        self.store.is_some()
    }

    pub fn load(&self) -> Result<RetainSnapshot, RuntimeError> {
        let Some(store) = self.store.as_ref() else {
            return Ok(RetainSnapshot::default());
        };
        store.load()
    }

    pub fn should_save(&self, now: Duration) -> bool {
        let Some(interval) = self.save_interval else {
            return false;
        };
        if !self.dirty {
            return false;
        }
        if interval.as_nanos() <= 0 {
            return true;
        }
        let elapsed = now.as_nanos().saturating_sub(self.last_save.as_nanos());
        elapsed >= interval.as_nanos()
    }

    pub fn save_snapshot(
        &mut self,
        snapshot: RetainSnapshot,
        now: Duration,
    ) -> Result<(), RuntimeError> {
        let Some(store) = self.store.as_ref() else {
            return Ok(());
        };
        if self.last_snapshot.as_ref() == Some(&snapshot) {
            self.dirty = false;
            self.last_save = now;
            return Ok(());
        }
        store.store(&snapshot)?;
        self.last_snapshot = Some(snapshot);
        self.dirty = false;
        self.last_save = now;
        Ok(())
    }
}

impl RetainSnapshot {
    pub fn from_runtime(runtime: &Runtime) -> Self {
        runtime.retain_snapshot()
    }
}

/// File-based retain store.
#[derive(Debug, Clone)]
pub struct FileRetainStore {
    path: PathBuf,
}

impl FileRetainStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), RuntimeError> {
        let mut file = fs::File::create(path)
            .map_err(|err| RuntimeError::RetainStore(format!("create {path:?}: {err}").into()))?;
        file.write_all(bytes)
            .map_err(|err| RuntimeError::RetainStore(format!("write {path:?}: {err}").into()))
    }

    fn read_bytes(path: &Path) -> Result<Vec<u8>, RuntimeError> {
        let mut file = fs::File::open(path)
            .map_err(|err| RuntimeError::RetainStore(format!("open {path:?}: {err}").into()))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|err| RuntimeError::RetainStore(format!("read {path:?}: {err}").into()))?;
        Ok(buf)
    }
}

impl RetainStore for FileRetainStore {
    fn load(&self) -> Result<RetainSnapshot, RuntimeError> {
        if !self.path.exists() {
            return Ok(RetainSnapshot::default());
        }
        let bytes = Self::read_bytes(&self.path)?;
        decode_snapshot(&bytes)
    }

    fn store(&self, snapshot: &RetainSnapshot) -> Result<(), RuntimeError> {
        let bytes = encode_snapshot(snapshot)?;
        Self::write_bytes(&self.path, &bytes)
    }
}

fn encode_snapshot(snapshot: &RetainSnapshot) -> Result<Vec<u8>, RuntimeError> {
    let mut out = Vec::new();
    out.extend_from_slice(RETAIN_MAGIC);
    out.extend_from_slice(&RETAIN_VERSION.to_le_bytes());
    out.extend_from_slice(&(snapshot.values.len() as u32).to_le_bytes());
    for (name, value) in &snapshot.values {
        encode_string(&mut out, name.as_str());
        encode_value(&mut out, value)?;
    }
    Ok(out)
}

fn decode_snapshot(bytes: &[u8]) -> Result<RetainSnapshot, RuntimeError> {
    let mut reader = RetainReader::new(bytes);
    let magic = reader.read_bytes(4)?;
    if magic != RETAIN_MAGIC {
        return Err(RuntimeError::RetainStore("invalid retain magic".into()));
    }
    let version = reader.read_u16()?;
    if version != RETAIN_VERSION {
        return Err(RuntimeError::RetainStore(
            format!("unsupported retain version {version}").into(),
        ));
    }
    let count = reader.read_u32()? as usize;
    let mut values = IndexMap::new();
    for _ in 0..count {
        let name = SmolStr::new(reader.read_string()?);
        let value = decode_value(&mut reader)?;
        values.insert(name, value);
    }
    Ok(RetainSnapshot { values })
}

#[derive(Debug, Clone, Copy)]
enum ValueTag {
    Bool = 1,
    SInt = 2,
    Int = 3,
    DInt = 4,
    LInt = 5,
    USInt = 6,
    UInt = 7,
    UDInt = 8,
    ULInt = 9,
    Real = 10,
    LReal = 11,
    Byte = 12,
    Word = 13,
    DWord = 14,
    LWord = 15,
    Time = 16,
    LTime = 17,
    Date = 18,
    LDate = 19,
    Tod = 20,
    LTod = 21,
    Dt = 22,
    Ldt = 23,
    String = 24,
    WString = 25,
    Char = 26,
    WChar = 27,
    Array = 28,
    Struct = 29,
    Enum = 30,
    Null = 31,
}

fn encode_value(out: &mut Vec<u8>, value: &Value) -> Result<(), RuntimeError> {
    match value {
        Value::Bool(v) => {
            out.push(ValueTag::Bool as u8);
            out.push(u8::from(*v));
        }
        Value::SInt(v) => {
            out.push(ValueTag::SInt as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Int(v) => {
            out.push(ValueTag::Int as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::DInt(v) => {
            out.push(ValueTag::DInt as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::LInt(v) => {
            out.push(ValueTag::LInt as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::USInt(v) => {
            out.push(ValueTag::USInt as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::UInt(v) => {
            out.push(ValueTag::UInt as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::UDInt(v) => {
            out.push(ValueTag::UDInt as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::ULInt(v) => {
            out.push(ValueTag::ULInt as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Real(v) => {
            out.push(ValueTag::Real as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::LReal(v) => {
            out.push(ValueTag::LReal as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Byte(v) => {
            out.push(ValueTag::Byte as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Word(v) => {
            out.push(ValueTag::Word as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::DWord(v) => {
            out.push(ValueTag::DWord as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::LWord(v) => {
            out.push(ValueTag::LWord as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Time(v) => {
            out.push(ValueTag::Time as u8);
            out.extend_from_slice(&v.as_nanos().to_le_bytes());
        }
        Value::LTime(v) => {
            out.push(ValueTag::LTime as u8);
            out.extend_from_slice(&v.as_nanos().to_le_bytes());
        }
        Value::Date(v) => {
            out.push(ValueTag::Date as u8);
            out.extend_from_slice(&v.ticks().to_le_bytes());
        }
        Value::LDate(v) => {
            out.push(ValueTag::LDate as u8);
            out.extend_from_slice(&v.nanos().to_le_bytes());
        }
        Value::Tod(v) => {
            out.push(ValueTag::Tod as u8);
            out.extend_from_slice(&v.ticks().to_le_bytes());
        }
        Value::LTod(v) => {
            out.push(ValueTag::LTod as u8);
            out.extend_from_slice(&v.nanos().to_le_bytes());
        }
        Value::Dt(v) => {
            out.push(ValueTag::Dt as u8);
            out.extend_from_slice(&v.ticks().to_le_bytes());
        }
        Value::Ldt(v) => {
            out.push(ValueTag::Ldt as u8);
            out.extend_from_slice(&v.nanos().to_le_bytes());
        }
        Value::String(v) => {
            out.push(ValueTag::String as u8);
            encode_string(out, v.as_str());
        }
        Value::WString(v) => {
            out.push(ValueTag::WString as u8);
            encode_string(out, v);
        }
        Value::Char(v) => {
            out.push(ValueTag::Char as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::WChar(v) => {
            out.push(ValueTag::WChar as u8);
            out.extend_from_slice(&v.to_le_bytes());
        }
        Value::Array(array) => {
            out.push(ValueTag::Array as u8);
            out.extend_from_slice(&(array.elements.len() as u32).to_le_bytes());
            out.extend_from_slice(&(array.dimensions.len() as u32).to_le_bytes());
            for (lower, upper) in &array.dimensions {
                out.extend_from_slice(&lower.to_le_bytes());
                out.extend_from_slice(&upper.to_le_bytes());
            }
            for element in &array.elements {
                encode_value(out, element)?;
            }
        }
        Value::Struct(struct_value) => {
            out.push(ValueTag::Struct as u8);
            encode_string(out, struct_value.type_name.as_str());
            out.extend_from_slice(&(struct_value.fields.len() as u32).to_le_bytes());
            for (name, field) in &struct_value.fields {
                encode_string(out, name.as_str());
                encode_value(out, field)?;
            }
        }
        Value::Enum(enum_value) => {
            out.push(ValueTag::Enum as u8);
            encode_string(out, enum_value.type_name.as_str());
            encode_string(out, enum_value.variant_name.as_str());
            out.extend_from_slice(&enum_value.numeric_value.to_le_bytes());
        }
        Value::Null => {
            out.push(ValueTag::Null as u8);
        }
        Value::Reference(_) | Value::Instance(_) => {
            return Err(RuntimeError::RetainStore(
                "cannot retain reference/instance values".into(),
            ));
        }
    }
    Ok(())
}

fn decode_value(reader: &mut RetainReader<'_>) -> Result<Value, RuntimeError> {
    let tag = reader.read_u8()?;
    let value = match tag {
        x if x == ValueTag::Bool as u8 => Value::Bool(reader.read_u8()? != 0),
        x if x == ValueTag::SInt as u8 => Value::SInt(reader.read_i8()?),
        x if x == ValueTag::Int as u8 => Value::Int(reader.read_i16()?),
        x if x == ValueTag::DInt as u8 => Value::DInt(reader.read_i32()?),
        x if x == ValueTag::LInt as u8 => Value::LInt(reader.read_i64()?),
        x if x == ValueTag::USInt as u8 => Value::USInt(reader.read_u8()?),
        x if x == ValueTag::UInt as u8 => Value::UInt(reader.read_u16()?),
        x if x == ValueTag::UDInt as u8 => Value::UDInt(reader.read_u32()?),
        x if x == ValueTag::ULInt as u8 => Value::ULInt(reader.read_u64()?),
        x if x == ValueTag::Real as u8 => Value::Real(reader.read_f32()?),
        x if x == ValueTag::LReal as u8 => Value::LReal(reader.read_f64()?),
        x if x == ValueTag::Byte as u8 => Value::Byte(reader.read_u8()?),
        x if x == ValueTag::Word as u8 => Value::Word(reader.read_u16()?),
        x if x == ValueTag::DWord as u8 => Value::DWord(reader.read_u32()?),
        x if x == ValueTag::LWord as u8 => Value::LWord(reader.read_u64()?),
        x if x == ValueTag::Time as u8 => Value::Time(Duration::from_nanos(reader.read_i64()?)),
        x if x == ValueTag::LTime as u8 => Value::LTime(Duration::from_nanos(reader.read_i64()?)),
        x if x == ValueTag::Date as u8 => Value::Date(DateValue::new(reader.read_i64()?)),
        x if x == ValueTag::LDate as u8 => Value::LDate(LDateValue::new(reader.read_i64()?)),
        x if x == ValueTag::Tod as u8 => Value::Tod(TimeOfDayValue::new(reader.read_i64()?)),
        x if x == ValueTag::LTod as u8 => Value::LTod(LTimeOfDayValue::new(reader.read_i64()?)),
        x if x == ValueTag::Dt as u8 => Value::Dt(DateTimeValue::new(reader.read_i64()?)),
        x if x == ValueTag::Ldt as u8 => Value::Ldt(LDateTimeValue::new(reader.read_i64()?)),
        x if x == ValueTag::String as u8 => Value::String(SmolStr::new(reader.read_string()?)),
        x if x == ValueTag::WString as u8 => Value::WString(reader.read_string()?),
        x if x == ValueTag::Char as u8 => Value::Char(reader.read_u8()?),
        x if x == ValueTag::WChar as u8 => Value::WChar(reader.read_u16()?),
        x if x == ValueTag::Array as u8 => {
            let len = reader.read_u32()? as usize;
            let dims = reader.read_u32()? as usize;
            let mut dimensions = Vec::with_capacity(dims);
            for _ in 0..dims {
                dimensions.push((reader.read_i64()?, reader.read_i64()?));
            }
            let mut elements = Vec::with_capacity(len);
            for _ in 0..len {
                elements.push(decode_value(reader)?);
            }
            Value::Array(ArrayValue {
                elements,
                dimensions,
            })
        }
        x if x == ValueTag::Struct as u8 => {
            let type_name = SmolStr::new(reader.read_string()?);
            let count = reader.read_u32()? as usize;
            let mut fields = IndexMap::new();
            for _ in 0..count {
                let name = SmolStr::new(reader.read_string()?);
                let value = decode_value(reader)?;
                fields.insert(name, value);
            }
            Value::Struct(StructValue { type_name, fields })
        }
        x if x == ValueTag::Enum as u8 => {
            let type_name = SmolStr::new(reader.read_string()?);
            let variant_name = SmolStr::new(reader.read_string()?);
            let numeric_value = reader.read_i64()?;
            Value::Enum(EnumValue {
                type_name,
                variant_name,
                numeric_value,
            })
        }
        x if x == ValueTag::Null as u8 => Value::Null,
        _ => return Err(RuntimeError::RetainStore("unknown retain value tag".into())),
    };
    Ok(value)
}

fn encode_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

struct RetainReader<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> RetainReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], RuntimeError> {
        let end = self.offset.saturating_add(len);
        if end > self.data.len() {
            return Err(RuntimeError::RetainStore("retain data truncated".into()));
        }
        let slice = &self.data[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, RuntimeError> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, RuntimeError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, RuntimeError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, RuntimeError> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_i8(&mut self) -> Result<i8, RuntimeError> {
        Ok(self.read_u8()? as i8)
    }

    fn read_i16(&mut self) -> Result<i16, RuntimeError> {
        Ok(self.read_u16()? as i16)
    }

    fn read_i32(&mut self) -> Result<i32, RuntimeError> {
        Ok(self.read_u32()? as i32)
    }

    fn read_i64(&mut self) -> Result<i64, RuntimeError> {
        Ok(self.read_u64()? as i64)
    }

    fn read_f32(&mut self) -> Result<f32, RuntimeError> {
        let bytes = self.read_bytes(4)?;
        Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn read_f64(&mut self) -> Result<f64, RuntimeError> {
        let bytes = self.read_bytes(8)?;
        Ok(f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn read_string(&mut self) -> Result<String, RuntimeError> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| RuntimeError::RetainStore("invalid utf-8 in retain".into()))
    }
}
