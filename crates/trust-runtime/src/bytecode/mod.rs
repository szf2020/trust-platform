//! Bytecode container format and metadata.

#![allow(missing_docs)]

mod decode;
mod encode;
mod encoder;
mod format;
mod metadata;
mod reader;
mod util;
mod validate;

pub use format::*;
