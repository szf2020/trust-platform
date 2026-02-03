//! Debug hook trait.

#![allow(missing_docs)]

use crate::eval::EvalContext;

use super::SourceLocation;

/// Debug hooks for statement-level instrumentation.
pub trait DebugHook {
    /// Called before a statement executes.
    fn on_statement(&mut self, location: Option<&SourceLocation>, call_depth: u32);

    /// Called before a statement executes with access to the eval context.
    fn on_statement_with_context(
        &mut self,
        _ctx: &mut EvalContext<'_>,
        location: Option<&SourceLocation>,
        call_depth: u32,
    ) {
        self.on_statement(location, call_depth);
    }
}

/// No-op debug hook.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopDebugHook;

impl DebugHook for NoopDebugHook {
    fn on_statement(&mut self, _location: Option<&SourceLocation>, _call_depth: u32) {}
}
