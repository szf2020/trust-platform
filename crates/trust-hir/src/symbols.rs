//! Symbol table and symbol definitions.
//!
//! This module provides the symbol table that tracks all declarations
//! in a Structured Text program.

mod builtins;
mod defs;
mod helpers;
mod table;

pub use defs::*;
pub use table::SymbolTable;
