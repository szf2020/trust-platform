//! Fault state management.

use crate::error::RuntimeError;
use crate::watchdog::{FaultDecision, FaultPolicy};

pub(super) struct FaultSubsystem {
    policy: FaultPolicy,
    faulted: bool,
    last_fault: Option<RuntimeError>,
}

impl FaultSubsystem {
    pub(super) fn new() -> Self {
        Self {
            policy: FaultPolicy::Halt,
            faulted: false,
            last_fault: None,
        }
    }

    pub(super) fn policy(&self) -> FaultPolicy {
        self.policy
    }

    pub(super) fn set_policy(&mut self, policy: FaultPolicy) {
        self.policy = policy;
    }

    pub(super) fn decision(&self) -> FaultDecision {
        FaultDecision::from_fault_policy(self.policy)
    }

    pub(super) fn record(&mut self, err: RuntimeError) {
        self.faulted = true;
        self.last_fault = Some(err);
    }

    pub(super) fn clear(&mut self) {
        self.faulted = false;
        self.last_fault = None;
    }

    pub(super) fn is_faulted(&self) -> bool {
        self.faulted
    }

    pub(super) fn last_fault(&self) -> Option<&RuntimeError> {
        self.last_fault.as_ref()
    }
}
