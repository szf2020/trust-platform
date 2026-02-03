//! Stop event coordination (ordering + filtering).

use std::io::BufWriter;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc::Receiver, Arc, Mutex};
use std::thread::{self, JoinHandle};

use trust_runtime::debug::{DebugControl, DebugStop, DebugStopReason};

use crate::protocol::{
    Event, InvalidatedEventBody, MessageType, OutputEventBody, StoppedEventBody,
};

use super::protocol_io::write_protocol_log;
use super::StopGate;

/// Coordinates stop ordering + filtering.
pub struct StopCoordinator {
    stop_gate: StopGate,
    pause_expected: Arc<AtomicBool>,
    stop_control: DebugControl,
    writer: Arc<Mutex<BufWriter<std::io::Stdout>>>,
    logger: Option<Arc<Mutex<BufWriter<std::fs::File>>>>,
    seq: Arc<AtomicU32>,
}

impl StopCoordinator {
    pub fn new(
        stop_gate: StopGate,
        pause_expected: Arc<AtomicBool>,
        stop_control: DebugControl,
        writer: Arc<Mutex<BufWriter<std::io::Stdout>>>,
        logger: Option<Arc<Mutex<BufWriter<std::fs::File>>>>,
        seq: Arc<AtomicU32>,
    ) -> Self {
        Self {
            stop_gate,
            pause_expected,
            stop_control,
            writer,
            logger,
            seq,
        }
    }

    pub fn spawn(self, stop_rx: Receiver<DebugStop>) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(stop) = stop_rx.recv() {
                self.stop_gate.wait_clear();
                if !self.should_emit_stop(&stop) {
                    continue;
                }
                if !self.emit_stop(stop) {
                    break;
                }
            }
        })
    }

    fn should_emit_stop(&self, stop: &DebugStop) -> bool {
        match stop.reason {
            DebugStopReason::Pause | DebugStopReason::Entry => {
                if !self.pause_expected.swap(false, Ordering::SeqCst) {
                    return false;
                }
            }
            DebugStopReason::Breakpoint | DebugStopReason::Step => {
                self.pause_expected.store(false, Ordering::SeqCst);
            }
        }
        if matches!(stop.reason, DebugStopReason::Breakpoint) {
            let Some(location) = stop.location else {
                return false;
            };
            let Some(generation) = stop.breakpoint_generation else {
                return false;
            };
            if self.stop_control.breakpoint_generation(location.file_id) != Some(generation) {
                return false;
            }
        }
        true
    }

    fn emit_stop(&self, stop: DebugStop) -> bool {
        let reason = match stop.reason {
            DebugStopReason::Breakpoint => "breakpoint",
            DebugStopReason::Step => "step",
            DebugStopReason::Pause => "pause",
            DebugStopReason::Entry => "entry",
        };
        let thread_id = stop.thread_id.or(Some(1));
        let output_body = OutputEventBody {
            output: format!(
                "[trust-debug] stopped: reason={} thread_id={}\n",
                reason,
                thread_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            ),
            category: Some("console".to_string()),
            source: None,
            line: None,
            column: None,
        };
        let output_event = Event {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            message_type: MessageType::Event,
            event: "output".to_string(),
            body: Some(output_body),
        };
        let all_threads_stopped = self.stop_control.target_thread().is_none();
        let body = StoppedEventBody {
            reason: reason.to_string(),
            thread_id,
            all_threads_stopped: Some(all_threads_stopped),
        };
        let event = Event {
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
            message_type: MessageType::Event,
            event: "stopped".to_string(),
            body: Some(body),
        };
        let output_serialized = match serde_json::to_string(&output_event) {
            Ok(serialized) => serialized,
            Err(_) => return true,
        };
        let serialized = match serde_json::to_string(&event) {
            Ok(serialized) => serialized,
            Err(_) => return true,
        };
        if let Some(logger) = &self.logger {
            let _ = write_protocol_log(logger, "->", &output_serialized);
            let _ = write_protocol_log(logger, "->", &serialized);
        }
        if super::protocol_io::write_message_locked(&self.writer, &output_serialized).is_err() {
            return false;
        }
        if super::protocol_io::write_message_locked(&self.writer, &serialized).is_err() {
            return false;
        }
        if self.stop_control.take_watch_changed() {
            let body = InvalidatedEventBody {
                areas: Some(vec!["watch".to_string()]),
                thread_id,
                stack_frame_id: None,
            };
            let event = Event {
                seq: self.seq.fetch_add(1, Ordering::Relaxed),
                message_type: MessageType::Event,
                event: "invalidated".to_string(),
                body: Some(body),
            };
            let serialized = match serde_json::to_string(&event) {
                Ok(serialized) => serialized,
                Err(_) => return true,
            };
            if let Some(logger) = &self.logger {
                let _ = write_protocol_log(logger, "->", &serialized);
            }
            if super::protocol_io::write_message_locked(&self.writer, &serialized).is_err() {
                return false;
            }
        }
        true
    }
}
