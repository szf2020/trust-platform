use smol_str::SmolStr;

use crate::memory::MemoryLocation;

/// Reference path segment within composite values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RefSegment {
    Index(Vec<i64>),
    Field(SmolStr),
}

/// Reference to a value in memory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueRef {
    pub location: MemoryLocation,
    pub offset: usize,
    pub path: Vec<RefSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialAccess {
    Bit(u8),
    Byte(u8),
    Word(u8),
    DWord(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialAccessError {
    IndexOutOfBounds { index: i64, lower: i64, upper: i64 },
    TypeMismatch,
}

pub fn parse_partial_access(text: &str) -> Option<PartialAccess> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(stripped) = text.strip_prefix('%') {
        let mut chars = stripped.chars();
        let prefix = chars.next()?;
        let digits: String = chars.collect();
        let index = parse_access_index(&digits)?;
        return match prefix.to_ascii_uppercase() {
            'X' => Some(PartialAccess::Bit(index)),
            'B' => Some(PartialAccess::Byte(index)),
            'W' => Some(PartialAccess::Word(index)),
            'D' => Some(PartialAccess::DWord(index)),
            _ => None,
        };
    }
    if text.chars().all(|c| c.is_ascii_digit() || c == '_') {
        let index = parse_access_index(text)?;
        return Some(PartialAccess::Bit(index));
    }
    None
}

fn parse_access_index(text: &str) -> Option<u8> {
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    let value: u64 = cleaned.parse().ok()?;
    u8::try_from(value).ok()
}
