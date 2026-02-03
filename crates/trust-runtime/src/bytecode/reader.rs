//! Bytecode reader utilities.

#![allow(missing_docs)]

use super::BytecodeError;

#[derive(Debug, Clone)]
pub(crate) struct BytecodeReader<'a> {
    data: &'a [u8],
    cursor: usize,
}

impl<'a> BytecodeReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, cursor: 0 }
    }

    pub(crate) fn pos(&self) -> usize {
        self.cursor
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.cursor)
    }

    pub(crate) fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], BytecodeError> {
        if self.cursor + len > self.data.len() {
            return Err(BytecodeError::UnexpectedEof);
        }
        let start = self.cursor;
        self.cursor += len;
        Ok(&self.data[start..start + len])
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8, BytecodeError> {
        Ok(self.read_bytes(1)?[0])
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16, BytecodeError> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32, BytecodeError> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64, BytecodeError> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    pub(crate) fn read_i32(&mut self) -> Result<i32, BytecodeError> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub(crate) fn read_i64(&mut self) -> Result<i64, BytecodeError> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }
}
