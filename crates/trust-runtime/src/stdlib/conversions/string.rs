use crate::error::RuntimeError;
use crate::value::Value;
use trust_hir::TypeId;

pub(super) fn convert_to_string(value: &Value, dst: TypeId) -> Result<Value, RuntimeError> {
    match dst {
        TypeId::STRING => match value {
            Value::String(s) => Ok(Value::String(s.clone())),
            Value::WString(s) => Ok(Value::String(s.clone().into())),
            Value::Char(c) => Ok(Value::String(((*c as char).to_string()).into())),
            Value::WChar(c) => {
                let ch = std::char::from_u32(*c as u32).ok_or(RuntimeError::TypeMismatch)?;
                Ok(Value::String(ch.to_string().into()))
            }
            _ => Err(RuntimeError::TypeMismatch),
        },
        TypeId::WSTRING => match value {
            Value::WString(s) => Ok(Value::WString(s.clone())),
            Value::String(s) => Ok(Value::WString(s.to_string())),
            Value::Char(c) => Ok(Value::WString((*c as char).to_string())),
            Value::WChar(c) => {
                let ch = std::char::from_u32(*c as u32).ok_or(RuntimeError::TypeMismatch)?;
                Ok(Value::WString(ch.to_string()))
            }
            _ => Err(RuntimeError::TypeMismatch),
        },
        _ => Err(RuntimeError::TypeMismatch),
    }
}

pub(super) fn convert_to_char(value: &Value, dst: TypeId) -> Result<Value, RuntimeError> {
    match dst {
        TypeId::CHAR => match value {
            Value::Char(c) => Ok(Value::Char(*c)),
            Value::WChar(c) => {
                if *c > u8::MAX as u16 {
                    return Err(RuntimeError::Overflow);
                }
                Ok(Value::Char(*c as u8))
            }
            Value::String(s) => string_to_char(s.as_str(), false),
            Value::WString(s) => string_to_char(s, false),
            _ => Err(RuntimeError::TypeMismatch),
        },
        TypeId::WCHAR => match value {
            Value::WChar(c) => Ok(Value::WChar(*c)),
            Value::Char(c) => Ok(Value::WChar(*c as u16)),
            Value::String(s) => string_to_char(s.as_str(), true),
            Value::WString(s) => string_to_char(s, true),
            _ => Err(RuntimeError::TypeMismatch),
        },
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn string_to_char(text: &str, wide: bool) -> Result<Value, RuntimeError> {
    let mut chars = text.chars();
    let ch = chars.next().ok_or(RuntimeError::TypeMismatch)?;
    if chars.next().is_some() {
        return Err(RuntimeError::TypeMismatch);
    }
    if wide {
        let code = u16::try_from(ch as u32).map_err(|_| RuntimeError::Overflow)?;
        Ok(Value::WChar(code))
    } else {
        let code = u8::try_from(ch as u32).map_err(|_| RuntimeError::Overflow)?;
        Ok(Value::Char(code))
    }
}
