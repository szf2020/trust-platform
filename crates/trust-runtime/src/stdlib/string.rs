//! String standard functions.

#![allow(missing_docs)]

use crate::error::RuntimeError;
use crate::stdlib::helpers::{require_arity, require_min, to_i64};
use crate::stdlib::StandardLibrary;
use crate::value::Value;
use smol_str::SmolStr;

pub fn register(lib: &mut StandardLibrary) {
    lib.register("LEN", &["IN"], len);
    lib.register("LEFT", &["IN", "L"], left);
    lib.register("RIGHT", &["IN", "L"], right);
    lib.register("MID", &["IN", "L", "P"], mid);
    lib.register_variadic("CONCAT", "IN", 1, 2, concat);
    lib.register("INSERT", &["IN1", "IN2", "P"], insert);
    lib.register("DELETE", &["IN", "L", "P"], delete);
    lib.register("REPLACE", &["IN1", "IN2", "L", "P"], replace);
    lib.register("FIND", &["IN1", "IN2"], find);
}

fn len(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 1)?;
    let length = match &args[0] {
        Value::String(value) => value.len(),
        Value::WString(value) => value.chars().count(),
        _ => return Err(RuntimeError::TypeMismatch),
    };
    if length > i16::MAX as usize {
        return Err(RuntimeError::Overflow);
    }
    Ok(Value::Int(length as i16))
}

fn left(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 2)?;
    let count = to_i64(&args[1])?;
    match &args[0] {
        Value::String(value) => {
            let bytes = value.as_bytes();
            let len = bytes.len();
            let take = if count <= 0 {
                0
            } else {
                count.min(len as i64) as usize
            };
            let result =
                std::str::from_utf8(&bytes[..take]).map_err(|_| RuntimeError::TypeMismatch)?;
            Ok(Value::String(SmolStr::new(result)))
        }
        Value::WString(value) => {
            let take = if count <= 0 { 0 } else { count as usize };
            let result: String = value.chars().take(take).collect();
            Ok(Value::WString(result))
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn right(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 2)?;
    let count = to_i64(&args[1])?;
    match &args[0] {
        Value::String(value) => {
            let bytes = value.as_bytes();
            let len = bytes.len();
            let take = if count <= 0 {
                0
            } else {
                count.min(len as i64) as usize
            };
            let start = len - take;
            let result =
                std::str::from_utf8(&bytes[start..]).map_err(|_| RuntimeError::TypeMismatch)?;
            Ok(Value::String(SmolStr::new(result)))
        }
        Value::WString(value) => {
            let total = value.chars().count();
            let take = if count <= 0 {
                0
            } else {
                count.min(total as i64) as usize
            };
            let start = total - take;
            let result: String = value.chars().skip(start).collect();
            Ok(Value::WString(result))
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn mid(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 3)?;
    let length = to_i64(&args[1])?;
    let position = to_i64(&args[2])?;
    match &args[0] {
        Value::String(value) => {
            let bytes = value.as_bytes();
            let len = bytes.len();
            let start = if position <= 1 {
                0
            } else {
                position as usize - 1
            };
            if start >= len || length <= 0 {
                return Ok(Value::String(SmolStr::new("")));
            }
            let end = (start as i64 + length).min(len as i64) as usize;
            let result =
                std::str::from_utf8(&bytes[start..end]).map_err(|_| RuntimeError::TypeMismatch)?;
            Ok(Value::String(SmolStr::new(result)))
        }
        Value::WString(value) => {
            let total = value.chars().count();
            let start = if position <= 1 {
                0
            } else {
                position as usize - 1
            };
            if start >= total || length <= 0 {
                return Ok(Value::WString(String::new()));
            }
            let end = (start as i64 + length).min(total as i64) as usize;
            let result: String = value.chars().skip(start).take(end - start).collect();
            Ok(Value::WString(result))
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn concat(args: &[Value]) -> Result<Value, RuntimeError> {
    require_min(args, 2)?;
    let is_wide = match &args[0] {
        Value::String(_) => false,
        Value::WString(_) => true,
        _ => return Err(RuntimeError::TypeMismatch),
    };
    if is_wide {
        let mut result = String::new();
        for value in args {
            match value {
                Value::WString(s) => result.push_str(s),
                _ => return Err(RuntimeError::TypeMismatch),
            }
        }
        Ok(Value::WString(result))
    } else {
        let mut result = String::new();
        for value in args {
            match value {
                Value::String(s) => result.push_str(s.as_str()),
                _ => return Err(RuntimeError::TypeMismatch),
            }
        }
        Ok(Value::String(SmolStr::new(result)))
    }
}

fn insert(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 3)?;
    let position = to_i64(&args[2])?;
    match (&args[0], &args[1]) {
        (Value::String(in1), Value::String(in2)) => {
            let bytes = in1.as_bytes();
            let len = bytes.len();
            let idx = if position <= 0 {
                0
            } else if position as usize >= len {
                len
            } else {
                position as usize
            };
            let mut result = Vec::with_capacity(len + in2.len());
            result.extend_from_slice(&bytes[..idx]);
            result.extend_from_slice(in2.as_bytes());
            result.extend_from_slice(&bytes[idx..]);
            let result = String::from_utf8(result).map_err(|_| RuntimeError::TypeMismatch)?;
            Ok(Value::String(SmolStr::new(result)))
        }
        (Value::WString(in1), Value::WString(in2)) => {
            let total = in1.chars().count();
            let idx = if position <= 0 {
                0
            } else if position as usize >= total {
                total
            } else {
                position as usize
            };
            let mut result = String::new();
            result.extend(in1.chars().take(idx));
            result.push_str(in2);
            result.extend(in1.chars().skip(idx));
            Ok(Value::WString(result))
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn delete(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 3)?;
    let length = to_i64(&args[1])?;
    let position = to_i64(&args[2])?;
    match &args[0] {
        Value::String(input) => {
            if length <= 0 {
                return Ok(Value::String(input.clone()));
            }
            let bytes = input.as_bytes();
            let len = bytes.len();
            let start = if position <= 1 {
                0
            } else {
                position as usize - 1
            };
            if start >= len {
                return Ok(Value::String(input.clone()));
            }
            let end = (start as i64 + length).min(len as i64) as usize;
            let mut result = Vec::with_capacity(len - (end - start));
            result.extend_from_slice(&bytes[..start]);
            result.extend_from_slice(&bytes[end..]);
            let result = String::from_utf8(result).map_err(|_| RuntimeError::TypeMismatch)?;
            Ok(Value::String(SmolStr::new(result)))
        }
        Value::WString(input) => {
            if length <= 0 {
                return Ok(Value::WString(input.clone()));
            }
            let total = input.chars().count();
            let start = if position <= 1 {
                0
            } else {
                position as usize - 1
            };
            if start >= total {
                return Ok(Value::WString(input.clone()));
            }
            let end = (start as i64 + length).min(total as i64) as usize;
            let mut result = String::new();
            result.extend(input.chars().take(start));
            result.extend(input.chars().skip(end));
            Ok(Value::WString(result))
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn replace(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 4)?;
    let length = to_i64(&args[2])?;
    let position = to_i64(&args[3])?;
    match (&args[0], &args[1]) {
        (Value::String(input), Value::String(repl)) => {
            let bytes = input.as_bytes();
            let len = bytes.len();
            let start = if position <= 1 {
                0
            } else {
                position as usize - 1
            };
            if start >= len {
                return Ok(Value::String(input.clone()));
            }
            let end = if length <= 0 {
                start
            } else {
                (start as i64 + length).min(len as i64) as usize
            };
            let mut result = Vec::with_capacity(len - (end - start) + repl.len());
            result.extend_from_slice(&bytes[..start]);
            result.extend_from_slice(repl.as_bytes());
            result.extend_from_slice(&bytes[end..]);
            let result = String::from_utf8(result).map_err(|_| RuntimeError::TypeMismatch)?;
            Ok(Value::String(SmolStr::new(result)))
        }
        (Value::WString(input), Value::WString(repl)) => {
            let total = input.chars().count();
            let start = if position <= 1 {
                0
            } else {
                position as usize - 1
            };
            if start >= total {
                return Ok(Value::WString(input.clone()));
            }
            let end = if length <= 0 {
                start
            } else {
                (start as i64 + length).min(total as i64) as usize
            };
            let mut result = String::new();
            result.extend(input.chars().take(start));
            result.push_str(repl);
            result.extend(input.chars().skip(end));
            Ok(Value::WString(result))
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn find(args: &[Value]) -> Result<Value, RuntimeError> {
    require_arity(args, 2)?;
    match (&args[0], &args[1]) {
        (Value::String(in1), Value::String(in2)) => {
            let pos = in1
                .as_str()
                .find(in2.as_str())
                .map(|idx| idx + 1)
                .unwrap_or(0);
            if pos > i16::MAX as usize {
                return Err(RuntimeError::Overflow);
            }
            Ok(Value::Int(pos as i16))
        }
        (Value::WString(in1), Value::WString(in2)) => {
            if in2.is_empty() {
                return Ok(Value::Int(1));
            }
            let byte_idx = in1.find(in2);
            let pos = match byte_idx {
                Some(idx) => in1[..idx].chars().count() + 1,
                None => 0,
            };
            if pos > i16::MAX as usize {
                return Err(RuntimeError::Overflow);
            }
            Ok(Value::Int(pos as i16))
        }
        _ => Err(RuntimeError::TypeMismatch),
    }
}
