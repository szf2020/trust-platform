//! Watchdog policy management.

use crate::watchdog::{FaultDecision, WatchdogPolicy};

pub(super) struct WatchdogSubsystem {
    policy: WatchdogPolicy,
}

impl WatchdogSubsystem {
    pub(super) fn new() -> Self {
        Self {
            policy: WatchdogPolicy::default(),
        }
    }

    pub(super) fn set_policy(&mut self, policy: WatchdogPolicy) {
        self.policy = policy;
    }

    pub(super) fn policy(&self) -> WatchdogPolicy {
        self.policy
    }

    pub(super) fn decision(&self) -> FaultDecision {
        FaultDecision::from_watchdog(self.policy.action)
    }
}
