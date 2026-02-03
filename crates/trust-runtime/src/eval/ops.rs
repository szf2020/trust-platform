//! Operator implementations.

#![allow(missing_docs)]

use crate::error::RuntimeError;
use crate::numeric::{
    numeric_kind, signed_from_i128, to_f64, to_i64, to_u64, unsigned_from_u128, wider_numeric,
    NumericKind,
};
use crate::value::{
    DateTimeProfile, DateTimeValue, DateValue, Duration, LDateTimeValue, LDateValue,
    LTimeOfDayValue, TimeOfDayValue, Value,
};

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Pos,
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    And,
    Or,
    Xor,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

pub fn apply_unary(op: UnaryOp, value: Value) -> Result<Value, RuntimeError> {
    match op {
        UnaryOp::Neg => match value {
            Value::SInt(v) => Ok(Value::SInt(-v)),
            Value::Int(v) => Ok(Value::Int(-v)),
            Value::DInt(v) => Ok(Value::DInt(-v)),
            Value::LInt(v) => Ok(Value::LInt(-v)),
            Value::Real(v) => Ok(Value::Real(-v)),
            Value::LReal(v) => Ok(Value::LReal(-v)),
            _ => Err(RuntimeError::TypeMismatch),
        },
        UnaryOp::Pos => Ok(value),
        UnaryOp::Not => match value {
            Value::Bool(v) => Ok(Value::Bool(!v)),
            Value::Byte(v) => Ok(Value::Byte(!v)),
            Value::Word(v) => Ok(Value::Word(!v)),
            Value::DWord(v) => Ok(Value::DWord(!v)),
            Value::LWord(v) => Ok(Value::LWord(!v)),
            _ => Err(RuntimeError::TypeMismatch),
        },
    }
}

pub fn apply_binary(
    op: BinaryOp,
    left: Value,
    right: Value,
    profile: &DateTimeProfile,
) -> Result<Value, RuntimeError> {
    if let Some(result) = time_arith(op, &left, &right, profile) {
        return result;
    }
    if let Some(result) = time_cmp(op, &left, &right) {
        return result;
    }
    match op {
        BinaryOp::And | BinaryOp::Or | BinaryOp::Xor => logical_or_bitwise(op, left, right),
        BinaryOp::Eq => numeric_eq(left, right, true),
        BinaryOp::Ne => numeric_eq(left, right, false),
        BinaryOp::Add => numeric_arith(op, left, right),
        BinaryOp::Sub => numeric_arith(op, left, right),
        BinaryOp::Mul => numeric_arith(op, left, right),
        BinaryOp::Div => numeric_arith(op, left, right),
        BinaryOp::Mod => numeric_arith(op, left, right),
        BinaryOp::Pow => numeric_arith(op, left, right),
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
            if let Some(result) = non_numeric_cmp(op, &left, &right) {
                return result;
            }
            numeric_cmp(op, left, right)
        }
    }
}

fn logical_or_bitwise(op: BinaryOp, left: Value, right: Value) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => {
            let result = match op {
                BinaryOp::And => a && b,
                BinaryOp::Or => a || b,
                BinaryOp::Xor => a ^ b,
                _ => return Err(RuntimeError::TypeMismatch),
            };
            Ok(Value::Bool(result))
        }
        (Value::Byte(a), Value::Byte(b)) => Ok(Value::Byte(bit_op(op, a, b)?)),
        (Value::Word(a), Value::Word(b)) => Ok(Value::Word(bit_op(op, a, b)?)),
        (Value::DWord(a), Value::DWord(b)) => Ok(Value::DWord(bit_op(op, a, b)?)),
        (Value::LWord(a), Value::LWord(b)) => Ok(Value::LWord(bit_op(op, a, b)?)),
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn bit_op<T>(op: BinaryOp, left: T, right: T) -> Result<T, RuntimeError>
where
    T: std::ops::BitAnd<Output = T>
        + std::ops::BitOr<Output = T>
        + std::ops::BitXor<Output = T>
        + Copy,
{
    let result = match op {
        BinaryOp::And => left & right,
        BinaryOp::Or => left | right,
        BinaryOp::Xor => left ^ right,
        _ => return Err(RuntimeError::TypeMismatch),
    };
    Ok(result)
}

fn numeric_eq(left: Value, right: Value, is_eq: bool) -> Result<Value, RuntimeError> {
    let left_nullish = matches!(left, Value::Null | Value::Reference(None));
    let right_nullish = matches!(right, Value::Null | Value::Reference(None));
    if left_nullish || right_nullish {
        let matches = left_nullish && right_nullish;
        return Ok(Value::Bool(if is_eq { matches } else { !matches }));
    }
    let left_kind = numeric_kind(&left);
    let right_kind = numeric_kind(&right);
    let Some(left_kind) = left_kind else {
        return Ok(Value::Bool(if is_eq {
            left == right
        } else {
            left != right
        }));
    };
    let Some(right_kind) = right_kind else {
        return Ok(Value::Bool(if is_eq {
            left == right
        } else {
            left != right
        }));
    };
    let target = wider_numeric(left_kind, right_kind);
    let matches = match target {
        NumericKind::Real | NumericKind::LReal => {
            let a = to_f64(&left)?;
            let b = to_f64(&right)?;
            a == b
        }
        NumericKind::SInt | NumericKind::Int | NumericKind::DInt | NumericKind::LInt => {
            let a = to_i64(&left)?;
            let b = to_i64(&right)?;
            a == b
        }
        NumericKind::USInt | NumericKind::UInt | NumericKind::UDInt | NumericKind::ULInt => {
            let a = to_u64(&left)?;
            let b = to_u64(&right)?;
            a == b
        }
    };
    Ok(Value::Bool(if is_eq { matches } else { !matches }))
}

fn non_numeric_cmp(
    op: BinaryOp,
    left: &Value,
    right: &Value,
) -> Option<Result<Value, RuntimeError>> {
    let result = match (left, right) {
        (Value::String(a), Value::String(b)) => ord_cmp(op, a.as_str(), b.as_str()),
        (Value::WString(a), Value::WString(b)) => ord_cmp(op, a.as_str(), b.as_str()),
        (Value::Char(a), Value::Char(b)) => ord_cmp(op, *a, *b),
        (Value::WChar(a), Value::WChar(b)) => ord_cmp(op, *a, *b),
        (Value::Bool(a), Value::Bool(b)) => ord_cmp(op, *a as u8, *b as u8),
        (Value::Byte(a), Value::Byte(b)) => ord_cmp(op, *a, *b),
        (Value::Word(a), Value::Word(b)) => ord_cmp(op, *a, *b),
        (Value::DWord(a), Value::DWord(b)) => ord_cmp(op, *a, *b),
        (Value::LWord(a), Value::LWord(b)) => ord_cmp(op, *a, *b),
        _ => return None,
    };
    Some(result)
}

fn ord_cmp<T: Ord>(op: BinaryOp, left: T, right: T) -> Result<Value, RuntimeError> {
    let result = match op {
        BinaryOp::Lt => left < right,
        BinaryOp::Le => left <= right,
        BinaryOp::Gt => left > right,
        BinaryOp::Ge => left >= right,
        _ => return Err(RuntimeError::TypeMismatch),
    };
    Ok(Value::Bool(result))
}

fn time_arith(
    op: BinaryOp,
    left: &Value,
    right: &Value,
    profile: &DateTimeProfile,
) -> Option<Result<Value, RuntimeError>> {
    match (left, right) {
        (Value::Time(lhs), Value::Time(rhs)) if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            return Some(time_duration_op(op, *lhs, *rhs).map(Value::Time));
        }
        (Value::LTime(lhs), Value::LTime(rhs)) if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            return Some(time_duration_op(op, *lhs, *rhs).map(Value::LTime));
        }
        (Value::Tod(lhs), Value::Time(rhs)) if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            return Some(time_of_day_with_time(op, *lhs, *rhs, profile).map(Value::Tod));
        }
        (Value::Time(lhs), Value::Tod(rhs)) if matches!(op, BinaryOp::Add) => {
            return Some(time_of_day_with_time(op, *rhs, *lhs, profile).map(Value::Tod));
        }
        (Value::LTod(lhs), Value::LTime(rhs)) if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            return Some(long_tod_with_time(op, *lhs, *rhs).map(Value::LTod));
        }
        (Value::LTime(lhs), Value::LTod(rhs)) if matches!(op, BinaryOp::Add) => {
            return Some(long_tod_with_time(op, *rhs, *lhs).map(Value::LTod));
        }
        (Value::Dt(lhs), Value::Time(rhs)) if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            return Some(datetime_with_time(op, *lhs, *rhs, profile).map(Value::Dt));
        }
        (Value::Time(lhs), Value::Dt(rhs)) if matches!(op, BinaryOp::Add) => {
            return Some(datetime_with_time(op, *rhs, *lhs, profile).map(Value::Dt));
        }
        (Value::Ldt(lhs), Value::LTime(rhs)) if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            return Some(long_datetime_with_time(op, *lhs, *rhs).map(Value::Ldt));
        }
        (Value::LTime(lhs), Value::Ldt(rhs)) if matches!(op, BinaryOp::Add) => {
            return Some(long_datetime_with_time(op, *rhs, *lhs).map(Value::Ldt));
        }
        (Value::Date(lhs), Value::Date(rhs)) if matches!(op, BinaryOp::Sub) => {
            return Some(date_diff(*lhs, *rhs, profile).map(Value::Time));
        }
        (Value::LDate(lhs), Value::LDate(rhs)) if matches!(op, BinaryOp::Sub) => {
            return Some(long_date_diff(*lhs, *rhs).map(Value::LTime));
        }
        (Value::Tod(lhs), Value::Tod(rhs)) if matches!(op, BinaryOp::Sub) => {
            return Some(tod_diff(*lhs, *rhs, profile).map(Value::Time));
        }
        (Value::LTod(lhs), Value::LTod(rhs)) if matches!(op, BinaryOp::Sub) => {
            return Some(long_tod_diff(*lhs, *rhs).map(Value::LTime));
        }
        (Value::Dt(lhs), Value::Dt(rhs)) if matches!(op, BinaryOp::Sub) => {
            return Some(dt_diff(*lhs, *rhs, profile).map(Value::Time));
        }
        (Value::Ldt(lhs), Value::Ldt(rhs)) if matches!(op, BinaryOp::Sub) => {
            return Some(long_dt_diff(*lhs, *rhs).map(Value::LTime));
        }
        _ => {}
    }

    if matches!(op, BinaryOp::Mul | BinaryOp::Div) {
        if let Some(result) = time_scale(op, left, right) {
            return Some(result);
        }
    }

    None
}

fn time_cmp(op: BinaryOp, left: &Value, right: &Value) -> Option<Result<Value, RuntimeError>> {
    if !matches!(
        op,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
    ) {
        return None;
    }
    let result = match (left, right) {
        (Value::Time(lhs), Value::Time(rhs)) => time_cmp_values(op, lhs.as_nanos(), rhs.as_nanos()),
        (Value::LTime(lhs), Value::LTime(rhs)) => {
            time_cmp_values(op, lhs.as_nanos(), rhs.as_nanos())
        }
        (Value::Date(lhs), Value::Date(rhs)) => time_cmp_values(op, lhs.ticks(), rhs.ticks()),
        (Value::LDate(lhs), Value::LDate(rhs)) => time_cmp_values(op, lhs.nanos(), rhs.nanos()),
        (Value::Tod(lhs), Value::Tod(rhs)) => time_cmp_values(op, lhs.ticks(), rhs.ticks()),
        (Value::LTod(lhs), Value::LTod(rhs)) => time_cmp_values(op, lhs.nanos(), rhs.nanos()),
        (Value::Dt(lhs), Value::Dt(rhs)) => time_cmp_values(op, lhs.ticks(), rhs.ticks()),
        (Value::Ldt(lhs), Value::Ldt(rhs)) => time_cmp_values(op, lhs.nanos(), rhs.nanos()),
        _ => return None,
    };
    Some(result.map(Value::Bool))
}

fn time_cmp_values(op: BinaryOp, lhs: i64, rhs: i64) -> Result<bool, RuntimeError> {
    let result = match op {
        BinaryOp::Lt => lhs < rhs,
        BinaryOp::Le => lhs <= rhs,
        BinaryOp::Gt => lhs > rhs,
        BinaryOp::Ge => lhs >= rhs,
        _ => return Err(RuntimeError::TypeMismatch),
    };
    Ok(result)
}

fn time_duration_op(op: BinaryOp, lhs: Duration, rhs: Duration) -> Result<Duration, RuntimeError> {
    let lhs = i128::from(lhs.as_nanos());
    let rhs = i128::from(rhs.as_nanos());
    let result = match op {
        BinaryOp::Add => lhs + rhs,
        BinaryOp::Sub => lhs - rhs,
        _ => return Err(RuntimeError::TypeMismatch),
    };
    let nanos = i64::try_from(result).map_err(|_| RuntimeError::Overflow)?;
    Ok(Duration::from_nanos(nanos))
}

fn time_of_day_with_time(
    op: BinaryOp,
    tod: TimeOfDayValue,
    time: Duration,
    profile: &DateTimeProfile,
) -> Result<TimeOfDayValue, RuntimeError> {
    let delta_ticks = duration_to_ticks(time, profile)?;
    let base = i128::from(tod.ticks());
    let result = match op {
        BinaryOp::Add => base + i128::from(delta_ticks),
        BinaryOp::Sub => base - i128::from(delta_ticks),
        _ => return Err(RuntimeError::TypeMismatch),
    };
    TimeOfDayValue::try_from_ticks(result).map_err(RuntimeError::from)
}

fn long_tod_with_time(
    op: BinaryOp,
    tod: LTimeOfDayValue,
    time: Duration,
) -> Result<LTimeOfDayValue, RuntimeError> {
    let base = i128::from(tod.nanos());
    let delta = i128::from(time.as_nanos());
    let result = match op {
        BinaryOp::Add => base + delta,
        BinaryOp::Sub => base - delta,
        _ => return Err(RuntimeError::TypeMismatch),
    };
    let nanos = i64::try_from(result).map_err(|_| RuntimeError::Overflow)?;
    Ok(LTimeOfDayValue::new(nanos))
}

fn datetime_with_time(
    op: BinaryOp,
    dt: DateTimeValue,
    time: Duration,
    profile: &DateTimeProfile,
) -> Result<DateTimeValue, RuntimeError> {
    let delta_ticks = duration_to_ticks(time, profile)?;
    let base = i128::from(dt.ticks());
    let result = match op {
        BinaryOp::Add => base + i128::from(delta_ticks),
        BinaryOp::Sub => base - i128::from(delta_ticks),
        _ => return Err(RuntimeError::TypeMismatch),
    };
    DateTimeValue::try_from_ticks(result).map_err(RuntimeError::from)
}

fn long_datetime_with_time(
    op: BinaryOp,
    dt: LDateTimeValue,
    time: Duration,
) -> Result<LDateTimeValue, RuntimeError> {
    let base = i128::from(dt.nanos());
    let delta = i128::from(time.as_nanos());
    let result = match op {
        BinaryOp::Add => base + delta,
        BinaryOp::Sub => base - delta,
        _ => return Err(RuntimeError::TypeMismatch),
    };
    let nanos = i64::try_from(result).map_err(|_| RuntimeError::Overflow)?;
    Ok(LDateTimeValue::new(nanos))
}

fn date_diff(
    lhs: DateValue,
    rhs: DateValue,
    profile: &DateTimeProfile,
) -> Result<Duration, RuntimeError> {
    let diff = i128::from(lhs.ticks()) - i128::from(rhs.ticks());
    ticks_to_duration(diff, profile)
}

fn long_date_diff(lhs: LDateValue, rhs: LDateValue) -> Result<Duration, RuntimeError> {
    let diff = i128::from(lhs.nanos()) - i128::from(rhs.nanos());
    let nanos = i64::try_from(diff).map_err(|_| RuntimeError::Overflow)?;
    Ok(Duration::from_nanos(nanos))
}

fn tod_diff(
    lhs: TimeOfDayValue,
    rhs: TimeOfDayValue,
    profile: &DateTimeProfile,
) -> Result<Duration, RuntimeError> {
    let diff = i128::from(lhs.ticks()) - i128::from(rhs.ticks());
    ticks_to_duration(diff, profile)
}

fn long_tod_diff(lhs: LTimeOfDayValue, rhs: LTimeOfDayValue) -> Result<Duration, RuntimeError> {
    let diff = i128::from(lhs.nanos()) - i128::from(rhs.nanos());
    let nanos = i64::try_from(diff).map_err(|_| RuntimeError::Overflow)?;
    Ok(Duration::from_nanos(nanos))
}

fn dt_diff(
    lhs: DateTimeValue,
    rhs: DateTimeValue,
    profile: &DateTimeProfile,
) -> Result<Duration, RuntimeError> {
    let diff = i128::from(lhs.ticks()) - i128::from(rhs.ticks());
    ticks_to_duration(diff, profile)
}

fn long_dt_diff(lhs: LDateTimeValue, rhs: LDateTimeValue) -> Result<Duration, RuntimeError> {
    let diff = i128::from(lhs.nanos()) - i128::from(rhs.nanos());
    let nanos = i64::try_from(diff).map_err(|_| RuntimeError::Overflow)?;
    Ok(Duration::from_nanos(nanos))
}

fn time_scale(op: BinaryOp, left: &Value, right: &Value) -> Option<Result<Value, RuntimeError>> {
    match (left, right) {
        (Value::Time(time), rhs) => {
            return Some(scale_duration(*time, rhs, op).map(Value::Time));
        }
        (lhs, Value::Time(time)) if matches!(op, BinaryOp::Mul) => {
            return Some(scale_duration(*time, lhs, op).map(Value::Time));
        }
        (Value::LTime(time), rhs) => {
            return Some(scale_duration(*time, rhs, op).map(Value::LTime));
        }
        (lhs, Value::LTime(time)) if matches!(op, BinaryOp::Mul) => {
            return Some(scale_duration(*time, lhs, op).map(Value::LTime));
        }
        _ => {}
    }
    None
}

fn scale_duration(time: Duration, factor: &Value, op: BinaryOp) -> Result<Duration, RuntimeError> {
    let factor = numeric_factor(factor)?;
    let nanos = i128::from(time.as_nanos());
    let result = match factor {
        NumericFactor::Integer(value) => match op {
            BinaryOp::Mul => nanos.checked_mul(value).ok_or(RuntimeError::Overflow)?,
            BinaryOp::Div => {
                if value == 0 {
                    return Err(RuntimeError::DivisionByZero);
                }
                nanos / value
            }
            _ => return Err(RuntimeError::TypeMismatch),
        },
        NumericFactor::Real(value) => {
            if matches!(op, BinaryOp::Div) && value == 0.0 {
                return Err(RuntimeError::DivisionByZero);
            }
            let result = match op {
                BinaryOp::Mul => (nanos as f64) * value,
                BinaryOp::Div => (nanos as f64) / value,
                _ => return Err(RuntimeError::TypeMismatch),
            };
            let truncated = result.trunc();
            if !truncated.is_finite() {
                return Err(RuntimeError::Overflow);
            }
            if truncated < i128::MIN as f64 || truncated > i128::MAX as f64 {
                return Err(RuntimeError::Overflow);
            }
            truncated as i128
        }
    };
    let nanos = i64::try_from(result).map_err(|_| RuntimeError::Overflow)?;
    Ok(Duration::from_nanos(nanos))
}

enum NumericFactor {
    Integer(i128),
    Real(f64),
}

fn numeric_factor(value: &Value) -> Result<NumericFactor, RuntimeError> {
    match value {
        Value::Real(v) => Ok(NumericFactor::Real(*v as f64)),
        Value::LReal(v) => Ok(NumericFactor::Real(*v)),
        Value::SInt(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        Value::Int(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        Value::DInt(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        Value::LInt(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        Value::USInt(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        Value::UInt(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        Value::UDInt(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        Value::ULInt(v) => Ok(NumericFactor::Integer(i128::from(*v))),
        _ => Err(RuntimeError::TypeMismatch),
    }
}

fn duration_to_ticks(time: Duration, profile: &DateTimeProfile) -> Result<i64, RuntimeError> {
    let resolution = profile.resolution.as_nanos();
    if resolution == 0 {
        return Err(RuntimeError::Overflow);
    }
    let ticks = i128::from(time.as_nanos()) / i128::from(resolution);
    i64::try_from(ticks).map_err(|_| RuntimeError::Overflow)
}

fn ticks_to_duration(ticks: i128, profile: &DateTimeProfile) -> Result<Duration, RuntimeError> {
    let nanos = ticks
        .checked_mul(i128::from(profile.resolution.as_nanos()))
        .ok_or(RuntimeError::Overflow)?;
    let nanos = i64::try_from(nanos).map_err(|_| RuntimeError::Overflow)?;
    Ok(Duration::from_nanos(nanos))
}
fn numeric_cmp(op: BinaryOp, left: Value, right: Value) -> Result<Value, RuntimeError> {
    let left_kind = numeric_kind(&left).ok_or(RuntimeError::TypeMismatch)?;
    let right_kind = numeric_kind(&right).ok_or(RuntimeError::TypeMismatch)?;
    let target = wider_numeric(left_kind, right_kind);
    let result = match target {
        NumericKind::Real | NumericKind::LReal => {
            let a = to_f64(&left)?;
            let b = to_f64(&right)?;
            match op {
                BinaryOp::Lt => a < b,
                BinaryOp::Le => a <= b,
                BinaryOp::Gt => a > b,
                BinaryOp::Ge => a >= b,
                _ => return Err(RuntimeError::TypeMismatch),
            }
        }
        NumericKind::SInt | NumericKind::Int | NumericKind::DInt | NumericKind::LInt => {
            let a = to_i64(&left)?;
            let b = to_i64(&right)?;
            match op {
                BinaryOp::Lt => a < b,
                BinaryOp::Le => a <= b,
                BinaryOp::Gt => a > b,
                BinaryOp::Ge => a >= b,
                _ => return Err(RuntimeError::TypeMismatch),
            }
        }
        NumericKind::USInt | NumericKind::UInt | NumericKind::UDInt | NumericKind::ULInt => {
            let a = to_u64(&left)?;
            let b = to_u64(&right)?;
            match op {
                BinaryOp::Lt => a < b,
                BinaryOp::Le => a <= b,
                BinaryOp::Gt => a > b,
                BinaryOp::Ge => a >= b,
                _ => return Err(RuntimeError::TypeMismatch),
            }
        }
    };
    Ok(Value::Bool(result))
}

fn numeric_arith(op: BinaryOp, left: Value, right: Value) -> Result<Value, RuntimeError> {
    let left_kind = numeric_kind(&left).ok_or(RuntimeError::TypeMismatch)?;
    let right_kind = numeric_kind(&right).ok_or(RuntimeError::TypeMismatch)?;
    let target = wider_numeric(left_kind, right_kind);
    match target {
        NumericKind::Real | NumericKind::LReal => {
            if matches!(op, BinaryOp::Mod) {
                return Err(RuntimeError::TypeMismatch);
            }
            let a = to_f64(&left)?;
            let b = to_f64(&right)?;
            if matches!(op, BinaryOp::Div) && b == 0.0 {
                return Err(RuntimeError::DivisionByZero);
            }
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => a / b,
                BinaryOp::Pow => a.powf(b),
                _ => return Err(RuntimeError::TypeMismatch),
            };
            if !result.is_finite() {
                return Err(RuntimeError::Overflow);
            }
            Ok(match target {
                NumericKind::Real => Value::Real(result as f32),
                NumericKind::LReal => Value::LReal(result),
                _ => unreachable!(),
            })
        }
        NumericKind::SInt | NumericKind::Int | NumericKind::DInt | NumericKind::LInt => {
            let a = i128::from(to_i64(&left)?);
            let b = i128::from(to_i64(&right)?);
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a - b,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => {
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero);
                    }
                    a / b
                }
                BinaryOp::Mod => {
                    if b == 0 {
                        return Err(RuntimeError::ModuloByZero);
                    }
                    a % b
                }
                BinaryOp::Pow => {
                    if b < 0 {
                        return Err(RuntimeError::TypeMismatch);
                    }
                    let exp = u32::try_from(b).map_err(|_| RuntimeError::Overflow)?;
                    a.checked_pow(exp).ok_or(RuntimeError::Overflow)?
                }
                _ => return Err(RuntimeError::TypeMismatch),
            };
            signed_from_i128(target, result)
        }
        NumericKind::USInt | NumericKind::UInt | NumericKind::UDInt | NumericKind::ULInt => {
            let a = u128::from(to_u64(&left)?);
            let b = u128::from(to_u64(&right)?);
            let result = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Sub => a.checked_sub(b).ok_or(RuntimeError::Overflow)?,
                BinaryOp::Mul => a * b,
                BinaryOp::Div => {
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero);
                    }
                    a / b
                }
                BinaryOp::Mod => {
                    if b == 0 {
                        return Err(RuntimeError::ModuloByZero);
                    }
                    a % b
                }
                BinaryOp::Pow => {
                    let exp = u32::try_from(b).map_err(|_| RuntimeError::Overflow)?;
                    a.checked_pow(exp).ok_or(RuntimeError::Overflow)?
                }
                _ => return Err(RuntimeError::TypeMismatch),
            };
            unsigned_from_u128(target, result)
        }
    }
}
