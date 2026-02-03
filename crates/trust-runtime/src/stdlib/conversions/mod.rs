//! Type conversion functions.

#![allow(missing_docs)]

mod bcd;
mod bitstring;
mod dispatch;
mod numeric;
mod spec;
mod string;
mod time;
mod util;

use super::StandardLibrary;
use crate::error::RuntimeError;
use crate::value::Value;

#[derive(Debug, Clone, Copy)]
enum ConversionMode {
    Round,
    Trunc,
}

pub fn register(_lib: &mut StandardLibrary) {}

pub fn is_conversion_name(name: &str) -> bool {
    spec::parse_conversion_spec(name).is_some()
}

pub fn call_conversion(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    let spec = spec::parse_conversion_spec(name)?;
    Some(dispatch::apply_conversion(spec, args))
}
