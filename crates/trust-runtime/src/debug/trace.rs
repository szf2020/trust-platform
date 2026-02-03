//! Debug trace helpers.

#![allow(missing_docs)]

use std::sync::OnceLock;

pub(crate) fn trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("ST_DEBUG_TRACE").is_some())
}

pub(crate) fn trace_debug(message: &str) {
    if trace_enabled() {
        eprintln!("[trust-runtime][debug] {message}");
    }
}
