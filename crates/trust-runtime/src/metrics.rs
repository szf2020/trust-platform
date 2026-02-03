//! Runtime metrics collection.

#![allow(missing_docs)]

use std::collections::HashMap;
use std::time::Instant;

use smol_str::SmolStr;

#[derive(Debug, Clone, Copy)]
pub struct CycleStats {
    pub min_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
    pub last_ms: f64,
    samples: u64,
}

impl CycleStats {
    pub fn record(&mut self, duration: std::time::Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        self.last_ms = ms;
        if self.samples == 0 {
            self.min_ms = ms;
            self.max_ms = ms;
            self.avg_ms = ms;
        } else {
            if ms < self.min_ms {
                self.min_ms = ms;
            }
            if ms > self.max_ms {
                self.max_ms = ms;
            }
            let total = self.avg_ms * self.samples as f64 + ms;
            self.avg_ms = total / (self.samples as f64 + 1.0);
        }
        self.samples = self.samples.saturating_add(1);
    }
}

impl Default for CycleStats {
    fn default() -> Self {
        Self {
            min_ms: 0.0,
            max_ms: 0.0,
            avg_ms: 0.0,
            last_ms: 0.0,
            samples: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TaskStats {
    pub min_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
    pub last_ms: f64,
    pub overruns: u64,
    samples: u64,
}

impl TaskStats {
    pub fn record(&mut self, duration: std::time::Duration) {
        let ms = duration.as_secs_f64() * 1000.0;
        self.last_ms = ms;
        if self.samples == 0 {
            self.min_ms = ms;
            self.max_ms = ms;
            self.avg_ms = ms;
        } else {
            if ms < self.min_ms {
                self.min_ms = ms;
            }
            if ms > self.max_ms {
                self.max_ms = ms;
            }
            let total = self.avg_ms * self.samples as f64 + ms;
            self.avg_ms = total / (self.samples as f64 + 1.0);
        }
        self.samples = self.samples.saturating_add(1);
    }

    pub fn record_overrun(&mut self, missed: u64) {
        self.overruns = self.overruns.saturating_add(missed);
    }
}

impl Default for TaskStats {
    fn default() -> Self {
        Self {
            min_ms: 0.0,
            max_ms: 0.0,
            avg_ms: 0.0,
            last_ms: 0.0,
            overruns: 0,
            samples: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeMetrics {
    start: Instant,
    pub cycle: CycleStats,
    pub tasks: HashMap<SmolStr, TaskStats>,
    pub faults: u64,
    pub overruns: u64,
}

impl RuntimeMetrics {
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            cycle: CycleStats::default(),
            tasks: HashMap::new(),
            faults: 0,
            overruns: 0,
        }
    }

    #[must_use]
    pub fn uptime_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn record_cycle(&mut self, duration: std::time::Duration) {
        self.cycle.record(duration);
    }

    pub fn record_task(&mut self, name: &SmolStr, duration: std::time::Duration) {
        let entry = self.tasks.entry(name.clone()).or_default();
        entry.record(duration);
    }

    pub fn record_overrun(&mut self, name: &SmolStr, missed: u64) {
        self.overruns = self.overruns.saturating_add(missed);
        let entry = self.tasks.entry(name.clone()).or_default();
        entry.record_overrun(missed);
    }

    pub fn record_fault(&mut self) {
        self.faults = self.faults.saturating_add(1);
    }

    #[must_use]
    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        let tasks = self
            .tasks
            .iter()
            .map(|(name, stats)| TaskStatsSnapshot {
                name: name.clone(),
                min_ms: stats.min_ms,
                max_ms: stats.max_ms,
                avg_ms: stats.avg_ms,
                last_ms: stats.last_ms,
                overruns: stats.overruns,
            })
            .collect();
        RuntimeMetricsSnapshot {
            uptime_ms: self.uptime_ms(),
            cycle: self.cycle,
            faults: self.faults,
            overruns: self.overruns,
            tasks,
        }
    }
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct TaskStatsSnapshot {
    pub name: SmolStr,
    pub min_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
    pub last_ms: f64,
    pub overruns: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeMetricsSnapshot {
    pub uptime_ms: u64,
    pub cycle: CycleStats,
    pub faults: u64,
    pub overruns: u64,
    pub tasks: Vec<TaskStatsSnapshot>,
}
