//! Shared numeric helpers.

#![allow(missing_docs)]

use crate::error::RuntimeError;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumericKind {
    SInt,
    Int,
    DInt,
    LInt,
    USInt,
    UInt,
    UDInt,
    ULInt,
    Real,
    LReal,
}

pub fn numeric_kind(value: &Value) -> Option<NumericKind> {
    match value {
        Value::SInt(_) => Some(NumericKind::SInt),
        Value::Int(_) => Some(NumericKind::Int),
        Value::DInt(_) => Some(NumericKind::DInt),
        Value::LInt(_) => Some(NumericKind::LInt),
        Value::USInt(_) => Some(NumericKind::USInt),
        Value::UInt(_) => Some(NumericKind::UInt),
        Value::UDInt(_) => Some(NumericKind::UDInt),
        Value::ULInt(_) => Some(NumericKind::ULInt),
        Value::Real(_) => Some(NumericKind::Real),
        Value::LReal(_) => Some(NumericKind::LReal),
        _ => None,
    }
}

pub fn wider_numeric(left: NumericKind, right: NumericKind) -> NumericKind {
    if numeric_rank(left) >= numeric_rank(right) {
        left
    } else {
        right
    }
}

fn numeric_rank(kind: NumericKind) -> u8 {
    match kind {
        NumericKind::SInt => 0,
        NumericKind::Int => 1,
        NumericKind::DInt => 2,
        NumericKind::LInt => 3,
        NumericKind::USInt => 4,
        NumericKind::UInt => 5,
        NumericKind::UDInt => 6,
        NumericKind::ULInt => 7,
        NumericKind::Real => 8,
        NumericKind::LReal => 9,
    }
}

pub fn to_i64(value: &Value) -> Result<i64, RuntimeError> {
    match value {
        Value::SInt(v) => Ok(*v as i64),
        Value::Int(v) => Ok(*v as i64),
        Value::DInt(v) => Ok(*v as i64),
        Value::LInt(v) => Ok(*v),
        Value::USInt(v) => Ok(*v as i64),
        Value::UInt(v) => Ok(*v as i64),
        Value::UDInt(v) => Ok(*v as i64),
        Value::ULInt(v) => i64::try_from(*v).map_err(|_| RuntimeError::Overflow),
        _ => Err(RuntimeError::TypeMismatch),
    }
}

pub fn to_u64(value: &Value) -> Result<u64, RuntimeError> {
    match value {
        Value::USInt(v) => Ok(*v as u64),
        Value::UInt(v) => Ok(*v as u64),
        Value::UDInt(v) => Ok(*v as u64),
        Value::ULInt(v) => Ok(*v),
        Value::SInt(v) => {
            if *v < 0 {
                Err(RuntimeError::TypeMismatch)
            } else {
                Ok(*v as u64)
            }
        }
        Value::Int(v) => {
            if *v < 0 {
                Err(RuntimeError::TypeMismatch)
            } else {
                Ok(*v as u64)
            }
        }
        Value::DInt(v) => {
            if *v < 0 {
                Err(RuntimeError::TypeMismatch)
            } else {
                Ok(*v as u64)
            }
        }
        Value::LInt(v) => {
            if *v < 0 {
                Err(RuntimeError::TypeMismatch)
            } else {
                Ok(*v as u64)
            }
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}

pub fn to_f64(value: &Value) -> Result<f64, RuntimeError> {
    match value {
        Value::Real(v) => Ok(*v as f64),
        Value::LReal(v) => Ok(*v),
        Value::SInt(v) => Ok(*v as f64),
        Value::Int(v) => Ok(*v as f64),
        Value::DInt(v) => Ok(*v as f64),
        Value::LInt(v) => Ok(*v as f64),
        Value::USInt(v) => Ok(*v as f64),
        Value::UInt(v) => Ok(*v as f64),
        Value::UDInt(v) => Ok(*v as f64),
        Value::ULInt(v) => Ok(*v as f64),
        _ => Err(RuntimeError::TypeMismatch),
    }
}

pub fn signed_from_i128(target: NumericKind, value: i128) -> Result<Value, RuntimeError> {
    match target {
        NumericKind::SInt => i8::try_from(value)
            .map(Value::SInt)
            .map_err(|_| RuntimeError::Overflow),
        NumericKind::Int => i16::try_from(value)
            .map(Value::Int)
            .map_err(|_| RuntimeError::Overflow),
        NumericKind::DInt => i32::try_from(value)
            .map(Value::DInt)
            .map_err(|_| RuntimeError::Overflow),
        NumericKind::LInt => i64::try_from(value)
            .map(Value::LInt)
            .map_err(|_| RuntimeError::Overflow),
        _ => Err(RuntimeError::TypeMismatch),
    }
}

pub fn unsigned_from_u128(target: NumericKind, value: u128) -> Result<Value, RuntimeError> {
    match target {
        NumericKind::USInt => u8::try_from(value)
            .map(Value::USInt)
            .map_err(|_| RuntimeError::Overflow),
        NumericKind::UInt => u16::try_from(value)
            .map(Value::UInt)
            .map_err(|_| RuntimeError::Overflow),
        NumericKind::UDInt => u32::try_from(value)
            .map(Value::UDInt)
            .map_err(|_| RuntimeError::Overflow),
        NumericKind::ULInt => u64::try_from(value)
            .map(Value::ULInt)
            .map_err(|_| RuntimeError::Overflow),
        _ => Err(RuntimeError::TypeMismatch),
    }
}
