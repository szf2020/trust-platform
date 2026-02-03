//! Debug data types.

#![allow(missing_docs)]

use smol_str::SmolStr;

use crate::eval::expr::Expr;
use crate::memory::VariableStorage;
use crate::value::Duration;

/// Source location for a statement or expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    /// Source file identifier (per lowering pass).
    pub file_id: u32,
    /// Byte offset at the start of the statement.
    pub start: u32,
    /// Byte offset at the end of the statement.
    pub end: u32,
}

impl SourceLocation {
    /// Create a new source location from byte offsets.
    #[must_use]
    pub fn new(file_id: u32, start: u32, end: u32) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }
}

/// Hit count conditions for breakpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitCondition {
    /// Break when hit count equals the target.
    Equal(u64),
    /// Break when hit count is at least the target.
    AtLeast(u64),
    /// Break when hit count is greater than the target.
    GreaterThan(u64),
}

impl HitCondition {
    /// Check whether the hit condition is satisfied.
    #[must_use]
    pub fn is_met(self, hits: u64) -> bool {
        match self {
            HitCondition::Equal(target) => hits == target,
            HitCondition::AtLeast(target) => hits >= target,
            HitCondition::GreaterThan(target) => hits > target,
        }
    }
}

/// Logpoint message fragments.
#[derive(Debug, Clone)]
pub enum LogFragment {
    /// Literal text.
    Text(String),
    /// Expression to evaluate.
    Expr(Expr),
}

/// Breakpoint definition with optional conditions.
#[derive(Debug, Clone)]
pub struct DebugBreakpoint {
    /// Resolved statement location for this breakpoint.
    pub location: SourceLocation,
    /// Optional condition expression evaluated at the statement boundary.
    pub condition: Option<Expr>,
    /// Optional hit count condition.
    pub hit_condition: Option<HitCondition>,
    /// Optional logpoint template fragments.
    pub log_message: Option<Vec<LogFragment>>,
    /// Current hit count for this breakpoint.
    pub hits: u64,
    /// Breakpoint generation (updated when setBreakpoints runs).
    pub generation: u64,
}

impl DebugBreakpoint {
    /// Create an unconditional breakpoint at a location.
    #[must_use]
    pub fn new(location: SourceLocation) -> Self {
        Self {
            location,
            condition: None,
            hit_condition: None,
            log_message: None,
            hits: 0,
            generation: 0,
        }
    }
}

/// Captured log output.
#[derive(Debug, Clone)]
pub struct DebugLog {
    /// Log message text.
    pub message: String,
    /// Optional source location for the log.
    pub location: Option<SourceLocation>,
}

/// Snapshot of runtime state at a stop.
#[derive(Debug, Clone)]
pub struct DebugSnapshot {
    /// Variable storage snapshot.
    pub storage: VariableStorage,
    /// Current runtime time.
    pub now: Duration,
}

/// Runtime scheduling and diagnostic events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    /// Cycle start event.
    CycleStart {
        /// Cycle counter value.
        cycle: u64,
        /// Time at cycle start.
        time: Duration,
    },
    /// Cycle end event.
    CycleEnd {
        /// Cycle counter value.
        cycle: u64,
        /// Time at cycle end.
        time: Duration,
    },
    /// Task execution start.
    TaskStart {
        /// Task name.
        name: SmolStr,
        /// Task priority (0 is highest).
        priority: u32,
        /// Time at task start.
        time: Duration,
    },
    /// Task execution end.
    TaskEnd {
        /// Task name.
        name: SmolStr,
        /// Task priority (0 is highest).
        priority: u32,
        /// Time at task end.
        time: Duration,
    },
    /// Task missed one or more periodic activations.
    TaskOverrun {
        /// Task name.
        name: SmolStr,
        /// Missed activation count.
        missed: u64,
        /// Time when the overrun was detected.
        time: Duration,
    },
    /// Resource fault event.
    Fault {
        /// Fault message.
        error: String,
        /// Time when the fault was recorded.
        time: Duration,
    },
}

/// Stop reason for debugger events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugStopReason {
    /// Paused due to a breakpoint.
    Breakpoint,
    /// Paused due to stepping.
    Step,
    /// Paused due to a user pause request.
    Pause,
    /// Paused due to stopOnEntry.
    Entry,
}

/// Notification emitted when execution stops.
#[derive(Debug, Clone)]
pub struct DebugStop {
    /// Reason for stopping.
    pub reason: DebugStopReason,
    /// Location where execution stopped (if known).
    pub location: Option<SourceLocation>,
    /// Thread/task id, if known.
    pub thread_id: Option<u32>,
    /// Breakpoint generation when the stop was emitted.
    pub breakpoint_generation: Option<u64>,
}
