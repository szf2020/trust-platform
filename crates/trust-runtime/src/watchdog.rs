//! Watchdog and fault policy helpers.

#![allow(missing_docs)]

use smol_str::SmolStr;

use crate::error::RuntimeError;
use crate::value::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogAction {
    Halt,
    SafeHalt,
    Restart,
}

impl WatchdogAction {
    pub fn parse(text: &str) -> Result<Self, RuntimeError> {
        match text.trim().to_ascii_lowercase().as_str() {
            "halt" => Ok(Self::Halt),
            "safe_halt" => Ok(Self::SafeHalt),
            "restart" => Ok(Self::Restart),
            _ => Err(RuntimeError::InvalidConfig(
                format!("invalid watchdog action '{text}'").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetainMode {
    None,
    File,
}

impl RetainMode {
    pub fn parse(text: &str) -> Result<Self, RuntimeError> {
        match text.trim().to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "file" => Ok(Self::File),
            _ => Err(RuntimeError::InvalidConfig(
                format!("invalid retain mode '{text}'").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPolicy {
    Halt,
    SafeHalt,
    Restart,
}

impl FaultPolicy {
    pub fn parse(text: &str) -> Result<Self, RuntimeError> {
        match text.trim().to_ascii_lowercase().as_str() {
            "halt" => Ok(Self::Halt),
            "safe_halt" => Ok(Self::SafeHalt),
            "restart" => Ok(Self::Restart),
            _ => Err(RuntimeError::InvalidConfig(
                format!("invalid fault policy '{text}'").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchdogPolicy {
    pub enabled: bool,
    pub timeout: Duration,
    pub action: WatchdogAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultAction {
    Halt,
    SafeHalt,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultDecision {
    pub action: FaultAction,
    pub apply_safe_state: bool,
}

impl FaultDecision {
    pub fn from_watchdog(action: WatchdogAction) -> Self {
        match action {
            WatchdogAction::Halt => Self {
                action: FaultAction::Halt,
                apply_safe_state: true,
            },
            WatchdogAction::SafeHalt => Self {
                action: FaultAction::SafeHalt,
                apply_safe_state: true,
            },
            WatchdogAction::Restart => Self {
                action: FaultAction::Restart,
                apply_safe_state: false,
            },
        }
    }

    pub fn from_fault_policy(policy: FaultPolicy) -> Self {
        match policy {
            FaultPolicy::Halt => Self {
                action: FaultAction::Halt,
                apply_safe_state: false,
            },
            FaultPolicy::SafeHalt => Self {
                action: FaultAction::SafeHalt,
                apply_safe_state: true,
            },
            FaultPolicy::Restart => Self {
                action: FaultAction::Restart,
                apply_safe_state: false,
            },
        }
    }
}

impl Default for WatchdogPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout: Duration::from_millis(0),
            action: WatchdogAction::SafeHalt,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FaultInfo {
    pub reason: SmolStr,
}
