//! Runtime control server (JSON IPC).

#![allow(missing_docs)]

mod transport;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use crate::config::ControlMode;
use crate::debug::{
    location_to_line_col, DebugBreakpoint, DebugControl, DebugScope, DebugSource, DebugVariable,
    DebugVariableHandles, VariableHandle,
};
use crate::error::RuntimeError;
use crate::io::{IoAddress, IoDriverHealth, IoDriverStatus, IoSnapshot};
use crate::metrics::RuntimeMetrics;
use crate::runtime::RuntimeMetadata;
use crate::scheduler::{ResourceCommand, ResourceControl};
use crate::settings::RuntimeSettings;
use crate::value::Value;
use crate::web::pairing::PairingStore;
use crate::RestartMode;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::json;
use smol_str::SmolStr;
use tracing::debug;

#[derive(Debug, Clone)]
pub enum ControlEndpoint {
    Tcp(SocketAddr),
    #[cfg(unix)]
    Unix(PathBuf),
}

impl ControlEndpoint {
    pub fn parse(text: &str) -> Result<Self, RuntimeError> {
        if let Some(rest) = text.strip_prefix("tcp://") {
            let addr = rest.parse::<SocketAddr>().map_err(|err| {
                RuntimeError::ControlError(format!("invalid tcp endpoint: {err}").into())
            })?;
            if !addr.ip().is_loopback() {
                return Err(RuntimeError::ControlError(
                    "tcp endpoint must be loopback (use unix:// for local sockets)".into(),
                ));
            }
            return Ok(Self::Tcp(addr));
        }
        #[cfg(unix)]
        if let Some(rest) = text.strip_prefix("unix://") {
            return Ok(Self::Unix(PathBuf::from(rest)));
        }
        Err(RuntimeError::ControlError(
            format!("unsupported endpoint '{text}'").into(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct ControlState {
    pub debug: DebugControl,
    pub resource: ResourceControl<crate::scheduler::StdClock>,
    pub metadata: Arc<Mutex<RuntimeMetadata>>,
    pub sources: SourceRegistry,
    pub io_snapshot: Arc<Mutex<Option<IoSnapshot>>>,
    pub pending_restart: Arc<Mutex<Option<RestartMode>>>,
    pub auth_token: Arc<Mutex<Option<SmolStr>>>,
    pub control_requires_auth: bool,
    pub control_mode: Arc<Mutex<ControlMode>>,
    pub audit_tx: Option<Sender<ControlAuditEvent>>,
    pub metrics: Arc<Mutex<RuntimeMetrics>>,
    pub events: Arc<Mutex<VecDeque<crate::debug::RuntimeEvent>>>,
    pub settings: Arc<Mutex<RuntimeSettings>>,
    pub resource_name: SmolStr,
    pub io_health: Arc<Mutex<Vec<IoDriverStatus>>>,
    pub debug_enabled: Arc<AtomicBool>,
    pub debug_variables: Arc<Mutex<DebugVariableHandles>>,
    pub pairing: Option<Arc<PairingStore>>,
}

#[derive(Debug, Clone)]
pub struct ControlAuditEvent {
    pub timestamp_ms: u128,
    pub request_id: u64,
    pub request_type: SmolStr,
    pub ok: bool,
    pub error: Option<SmolStr>,
    pub auth_present: bool,
    pub client: Option<SmolStr>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceRegistry {
    files: Vec<SourceFile>,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id: u32,
    pub path: PathBuf,
    pub text: String,
}

impl SourceRegistry {
    pub fn new(files: Vec<SourceFile>) -> Self {
        Self { files }
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn file_id_for_path(&self, path: &Path) -> Option<u32> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.id)
    }

    pub fn source_text(&self, file_id: u32) -> Option<&str> {
        self.files
            .iter()
            .find(|file| file.id == file_id)
            .map(|file| file.text.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[derive(Debug)]
pub struct ControlServer {
    endpoint: ControlEndpoint,
    #[allow(dead_code)]
    state: Arc<ControlState>,
}

impl ControlServer {
    pub fn start(
        endpoint: ControlEndpoint,
        state: Arc<ControlState>,
    ) -> Result<Self, RuntimeError> {
        transport::spawn_control_server(&endpoint, state.clone())?;
        Ok(Self { endpoint, state })
    }

    #[must_use]
    pub fn endpoint(&self) -> &ControlEndpoint {
        &self.endpoint
    }
}

pub(crate) fn handle_request_line(
    line: &str,
    state: &ControlState,
    client: Option<&str>,
) -> Option<String> {
    let response = match serde_json::from_str::<serde_json::Value>(line) {
        Ok(value) => handle_request_value(value, state, client),
        Err(err) => ControlResponse::error(0, format!("invalid request: {err}")),
    };
    serde_json::to_string(&response).ok()
}

pub(crate) fn handle_request_value(
    value: serde_json::Value,
    state: &ControlState,
    client: Option<&str>,
) -> ControlResponse {
    let request: ControlRequest = match serde_json::from_value(value) {
        Ok(req) => req,
        Err(err) => {
            let response = ControlResponse::error(0, format!("invalid request: {err}"));
            record_audit(
                state,
                0,
                SmolStr::new("invalid"),
                false,
                Some(SmolStr::new(format!("invalid request: {err}"))),
                false,
                client,
            );
            return response;
        }
    };
    if let Ok(guard) = state.auth_token.lock() {
        if let Some(expected) = guard.as_deref() {
            if request.auth.as_deref() != Some(expected) {
                let response = ControlResponse::error(request.id, "unauthorized".into());
                record_audit(
                    state,
                    request.id,
                    SmolStr::new(request.r#type.as_str()),
                    false,
                    Some(SmolStr::new("unauthorized")),
                    request.auth.is_some(),
                    client,
                );
                return response;
            }
        }
    }
    if !state.debug_enabled.load(Ordering::Relaxed) && is_debug_request(request.r#type.as_str()) {
        let response = ControlResponse::error(request.id, "debug disabled".into());
        record_audit(
            state,
            request.id,
            SmolStr::new(request.r#type.as_str()),
            false,
            Some(SmolStr::new("debug disabled")),
            request.auth.is_some(),
            client,
        );
        return response;
    }
    let response = match request.r#type.as_str() {
        "status" => handle_status(request.id, state),
        "health" => handle_health(request.id, state),
        "pause" => handle_pause(request.id, state),
        "resume" => handle_resume(request.id, state),
        "step_in" => handle_step(request.id, state, StepKind::In),
        "step_over" => handle_step(request.id, state, StepKind::Over),
        "step_out" => handle_step(request.id, state, StepKind::Out),
        "debug.state" => handle_debug_state(request.id, state),
        "debug.stops" => handle_debug_stops(request.id, state),
        "debug.stack" => handle_debug_stack(request.id, state),
        "debug.scopes" => handle_debug_scopes(request.id, request.params, state),
        "debug.variables" => handle_debug_variables(request.id, request.params, state),
        "debug.evaluate" => handle_debug_evaluate(request.id, request.params, state),
        "debug.breakpoint_locations" => {
            handle_debug_breakpoint_locations(request.id, request.params, state)
        }
        "tasks.stats" => handle_task_stats(request.id, state),
        "io.list" => handle_io_list(request.id, state),
        "events.tail" => handle_events_tail(request.id, request.params, state),
        "events" => handle_events_tail(request.id, request.params, state),
        "faults" => handle_faults(request.id, request.params, state),
        "config.get" => handle_config_get(request.id, state),
        "config.set" => handle_config_set(request.id, request.params, state),
        "breakpoints.set" => handle_breakpoints_set(request.id, request.params, state),
        "breakpoints.clear" => handle_breakpoints_clear(request.id, request.params, state),
        "breakpoints.list" => handle_breakpoints_list(request.id, state),
        "breakpoints.clear_all" => handle_breakpoints_clear_all(request.id, state),
        "breakpoints.clear_id" => handle_breakpoints_clear_id(request.id, request.params, state),
        "io.read" => handle_io_read(request.id, state),
        "io.write" => handle_io_write(request.id, request.params, state),
        "io.force" => handle_io_force(request.id, request.params, state),
        "io.unforce" => handle_io_unforce(request.id, request.params, state),
        "eval" => handle_eval(request.id, request.params, state),
        "set" => handle_set(request.id, request.params, state),
        "var.force" => handle_var_force(request.id, request.params, state),
        "var.unforce" => handle_var_unforce(request.id, request.params, state),
        "var.forced" => handle_var_forced(request.id, state),
        "shutdown" => handle_shutdown(request.id, state),
        "restart" => handle_restart(request.id, request.params, state),
        "bytecode.reload" => handle_bytecode_reload(request.id, request.params, state),
        "pair.start" => handle_pair_start(request.id, state),
        "pair.claim" => handle_pair_claim(request.id, request.params, state),
        "pair.list" => handle_pair_list(request.id, state),
        "pair.revoke" => handle_pair_revoke(request.id, request.params, state),
        _ => ControlResponse::error(request.id, "unsupported request".into()),
    };
    record_audit(
        state,
        request.id,
        SmolStr::new(request.r#type.as_str()),
        response.ok,
        response.error.as_ref().map(SmolStr::new),
        request.auth.is_some(),
        client,
    );
    response
}

fn record_audit(
    state: &ControlState,
    request_id: u64,
    request_type: SmolStr,
    ok: bool,
    error: Option<SmolStr>,
    auth_present: bool,
    client: Option<&str>,
) {
    let Some(sender) = &state.audit_tx else {
        return;
    };
    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let event = ControlAuditEvent {
        timestamp_ms,
        request_id,
        request_type,
        ok,
        error,
        auth_present,
        client: client.map(SmolStr::new),
    };
    let _ = sender.send(event);
}

fn is_debug_request(kind: &str) -> bool {
    matches!(
        kind,
        "pause"
            | "resume"
            | "step_in"
            | "step_over"
            | "step_out"
            | "breakpoints.set"
            | "breakpoints.clear"
            | "breakpoints.clear_all"
            | "breakpoints.clear_id"
            | "breakpoints.list"
            | "eval"
            | "set"
            | "var.force"
            | "var.unforce"
            | "var.forced"
            | "debug.state"
            | "debug.stops"
            | "debug.stack"
            | "debug.scopes"
            | "debug.variables"
            | "debug.evaluate"
            | "debug.breakpoint_locations"
    )
}

fn handle_status(id: u64, state: &ControlState) -> ControlResponse {
    let status = state.resource.state();
    let error = state.resource.last_error().map(|err| err.to_string());
    let io_health = state
        .io_health
        .lock()
        .ok()
        .map(|guard| guard.iter().map(io_health_to_json).collect::<Vec<_>>())
        .unwrap_or_default();
    let metrics = state
        .metrics
        .lock()
        .ok()
        .map(|guard| guard.snapshot())
        .unwrap_or_default();
    ControlResponse::ok(
        id,
        json!({
            "state": format!("{status:?}").to_ascii_lowercase(),
            "fault": error,
            "resource": state.resource_name.as_str(),
            "plc_name": state.resource_name.as_str(),
            "uptime_ms": metrics.uptime_ms,
            "debug_enabled": state.debug_enabled.load(Ordering::Relaxed),
            "control_mode": state
                .control_mode
                .lock()
                .map(|mode| format!("{:?}", *mode).to_ascii_lowercase())
                .unwrap_or_else(|_| "production".to_string()),
            "metrics": {
                "cycle_ms": {
                    "min": metrics.cycle.min_ms,
                    "avg": metrics.cycle.avg_ms,
                    "max": metrics.cycle.max_ms,
                    "last": metrics.cycle.last_ms,
                },
                "overruns": metrics.overruns,
                "faults": metrics.faults,
            },
            "io_drivers": io_health,
        }),
    )
}

fn handle_health(id: u64, state: &ControlState) -> ControlResponse {
    let status = state.resource.state();
    let error = state.resource.last_error().map(|err| err.to_string());
    let io_health = state
        .io_health
        .lock()
        .ok()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let has_faulted_driver = io_health
        .iter()
        .any(|entry| matches!(entry.health, IoDriverHealth::Faulted { .. }));
    let ok = matches!(
        status,
        crate::scheduler::ResourceState::Running
            | crate::scheduler::ResourceState::Ready
            | crate::scheduler::ResourceState::Paused
    ) && error.is_none()
        && !has_faulted_driver;
    ControlResponse::ok(
        id,
        json!({
            "ok": ok,
            "state": format!("{status:?}").to_ascii_lowercase(),
            "fault": error,
            "io_drivers": io_health.iter().map(io_health_to_json).collect::<Vec<_>>(),
        }),
    )
}

fn handle_task_stats(id: u64, state: &ControlState) -> ControlResponse {
    let metrics = state
        .metrics
        .lock()
        .ok()
        .map(|guard| guard.snapshot())
        .unwrap_or_default();
    let tasks = metrics
        .tasks
        .iter()
        .map(|task| {
            json!({
                "name": task.name.as_str(),
                "min_ms": task.min_ms,
                "avg_ms": task.avg_ms,
                "max_ms": task.max_ms,
                "last_ms": task.last_ms,
                "overruns": task.overruns,
            })
        })
        .collect::<Vec<_>>();
    ControlResponse::ok(id, json!({ "tasks": tasks }))
}

fn handle_io_list(id: u64, state: &ControlState) -> ControlResponse {
    let snapshot = match state.io_snapshot.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => None,
    };
    match snapshot {
        Some(snapshot) => ControlResponse::ok(id, snapshot.into_json()),
        None => ControlResponse::error(id, "no snapshot available".into()),
    }
}

fn handle_events_tail(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let limit = params
        .and_then(|value| value.get("limit").cloned())
        .and_then(|value| value.as_u64())
        .unwrap_or(50) as usize;
    let events = state
        .events
        .lock()
        .map(|guard| guard.iter().rev().take(limit).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let payload = events
        .into_iter()
        .map(runtime_event_to_json)
        .collect::<Vec<_>>();
    ControlResponse::ok(id, json!({ "events": payload }))
}

fn handle_faults(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let limit = params
        .and_then(|value| value.get("limit").cloned())
        .and_then(|value| value.as_u64())
        .unwrap_or(50) as usize;
    let events = state
        .events
        .lock()
        .map(|guard| guard.iter().rev().take(limit).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let faults = events
        .into_iter()
        .filter(|event| matches!(event, crate::debug::RuntimeEvent::Fault { .. }))
        .map(runtime_event_to_json)
        .collect::<Vec<_>>();
    ControlResponse::ok(id, json!({ "faults": faults }))
}

fn handle_config_get(id: u64, state: &ControlState) -> ControlResponse {
    let settings = match state.settings.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return ControlResponse::error(id, "settings unavailable".into()),
    };
    let auth = state.auth_token.lock().ok();
    let auth_set = auth
        .as_ref()
        .and_then(|value| value.as_ref())
        .map(|value| value.len())
        .unwrap_or(0);
    ControlResponse::ok(
        id,
        json!({
            "log.level": settings.log_level.as_str(),
            "watchdog.enabled": settings.watchdog.enabled,
            "watchdog.timeout_ms": settings.watchdog.timeout.as_millis(),
            "watchdog.action": format!("{:?}", settings.watchdog.action),
            "fault.policy": format!("{:?}", settings.fault_policy),
            "retain.mode": format!("{:?}", settings.retain_mode),
            "retain.save_interval_ms": settings.retain_save_interval.map(|val| val.as_millis()),
            "web.enabled": settings.web.enabled,
            "web.listen": settings.web.listen.as_str(),
            "web.auth": settings.web.auth.as_str(),
            "discovery.enabled": settings.discovery.enabled,
            "discovery.service_name": settings.discovery.service_name.as_str(),
            "discovery.advertise": settings.discovery.advertise,
            "discovery.interfaces": settings.discovery.interfaces.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            "mesh.enabled": settings.mesh.enabled,
            "mesh.listen": settings.mesh.listen.as_str(),
            "mesh.auth_token_set": settings.mesh.auth_token.as_ref().map(|t| t.len()).unwrap_or(0) > 0,
            "mesh.publish": settings.mesh.publish.iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            "mesh.subscribe": settings
                .mesh
                .subscribe
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_string(),
                        serde_json::Value::String(v.as_str().to_string()),
                    )
                })
                .collect::<serde_json::Map<_, _>>(),
            "control.auth_token_set": auth_set > 0,
            "control.auth_token_length": if auth_set > 0 { Some(auth_set) } else { None },
            "control.debug_enabled": state.debug_enabled.load(Ordering::Relaxed),
            "control.mode": state
                .control_mode
                .lock()
                .map(|mode| format!("{:?}", *mode))
                .unwrap_or_else(|_| "Production".to_string()),
        }),
    )
}

fn handle_config_set(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params = match params {
        Some(params) => params,
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let mut settings = match state.settings.lock() {
        Ok(guard) => guard,
        Err(_) => return ControlResponse::error(id, "settings unavailable".into()),
    };
    let mut updated = Vec::new();
    let mut restart_required = Vec::new();
    if let Some(value) = params.get("log.level").and_then(|v| v.as_str()) {
        settings.log_level = SmolStr::new(value);
        updated.push("log.level");
    }
    if let Some(enabled) = params.get("watchdog.enabled").and_then(|v| v.as_bool()) {
        settings.watchdog.enabled = enabled;
        updated.push("watchdog.enabled");
    }
    if let Some(timeout) = params.get("watchdog.timeout_ms").and_then(|v| v.as_i64()) {
        settings.watchdog.timeout = crate::value::Duration::from_millis(timeout);
        updated.push("watchdog.timeout_ms");
    }
    if let Some(action) = params.get("watchdog.action").and_then(|v| v.as_str()) {
        match crate::watchdog::WatchdogAction::parse(action) {
            Ok(action) => {
                settings.watchdog.action = action;
                updated.push("watchdog.action");
            }
            Err(err) => return ControlResponse::error(id, err.to_string()),
        }
    }
    if let Some(policy) = params.get("fault.policy").and_then(|v| v.as_str()) {
        match crate::watchdog::FaultPolicy::parse(policy) {
            Ok(policy) => {
                settings.fault_policy = policy;
                updated.push("fault.policy");
            }
            Err(err) => return ControlResponse::error(id, err.to_string()),
        }
    }
    if let Some(interval) = params
        .get("retain.save_interval_ms")
        .and_then(|v| v.as_i64())
    {
        settings.retain_save_interval = Some(crate::value::Duration::from_millis(interval));
        updated.push("retain.save_interval_ms");
    }
    if let Some(mode) = params.get("retain.mode").and_then(|v| v.as_str()) {
        match crate::watchdog::RetainMode::parse(mode) {
            Ok(mode) => {
                settings.retain_mode = mode;
                updated.push("retain.mode");
                restart_required.push("retain.mode");
            }
            Err(err) => return ControlResponse::error(id, err.to_string()),
        }
    }

    if let Some(enabled) = params.get("web.enabled").and_then(|v| v.as_bool()) {
        settings.web.enabled = enabled;
        updated.push("web.enabled");
        restart_required.push("web.enabled");
    }
    if let Some(listen) = params.get("web.listen").and_then(|v| v.as_str()) {
        settings.web.listen = SmolStr::new(listen);
        updated.push("web.listen");
        restart_required.push("web.listen");
    }
    if let Some(auth) = params.get("web.auth").and_then(|v| v.as_str()) {
        if auth.eq_ignore_ascii_case("token") {
            if let Ok(guard) = state.auth_token.lock() {
                if guard.is_none() {
                    return ControlResponse::error(
                        id,
                        "web.auth=token requires control auth token".into(),
                    );
                }
            }
        }
        settings.web.auth = SmolStr::new(auth);
        updated.push("web.auth");
        restart_required.push("web.auth");
    }
    if let Some(enabled) = params.get("discovery.enabled").and_then(|v| v.as_bool()) {
        settings.discovery.enabled = enabled;
        updated.push("discovery.enabled");
        restart_required.push("discovery.enabled");
    }
    if let Some(name) = params
        .get("discovery.service_name")
        .and_then(|v| v.as_str())
    {
        settings.discovery.service_name = SmolStr::new(name);
        updated.push("discovery.service_name");
        restart_required.push("discovery.service_name");
    }
    if let Some(advertise) = params.get("discovery.advertise").and_then(|v| v.as_bool()) {
        settings.discovery.advertise = advertise;
        updated.push("discovery.advertise");
        restart_required.push("discovery.advertise");
    }
    if let Some(interfaces) = params
        .get("discovery.interfaces")
        .and_then(|v| v.as_array())
    {
        settings.discovery.interfaces = interfaces
            .iter()
            .filter_map(|item| item.as_str())
            .map(SmolStr::new)
            .collect();
        updated.push("discovery.interfaces");
        restart_required.push("discovery.interfaces");
    }
    if let Some(enabled) = params.get("mesh.enabled").and_then(|v| v.as_bool()) {
        settings.mesh.enabled = enabled;
        updated.push("mesh.enabled");
        restart_required.push("mesh.enabled");
    }
    if let Some(listen) = params.get("mesh.listen").and_then(|v| v.as_str()) {
        settings.mesh.listen = SmolStr::new(listen);
        updated.push("mesh.listen");
        restart_required.push("mesh.listen");
    }
    if let Some(publish) = params.get("mesh.publish").and_then(|v| v.as_array()) {
        settings.mesh.publish = publish
            .iter()
            .filter_map(|item| item.as_str())
            .map(SmolStr::new)
            .collect();
        updated.push("mesh.publish");
        restart_required.push("mesh.publish");
    }
    if let Some(subscribe) = params.get("mesh.subscribe").and_then(|v| v.as_object()) {
        settings.mesh.subscribe = subscribe
            .iter()
            .map(|(k, v)| {
                (
                    SmolStr::new(k),
                    SmolStr::new(v.as_str().unwrap_or_default()),
                )
            })
            .collect();
        updated.push("mesh.subscribe");
        restart_required.push("mesh.subscribe");
    }
    if let Some(value) = params.get("mesh.auth_token") {
        if value.is_null() {
            settings.mesh.auth_token = None;
            updated.push("mesh.auth_token");
            restart_required.push("mesh.auth_token");
        } else if let Some(token) = value.as_str() {
            if token.trim().is_empty() {
                return ControlResponse::error(id, "mesh auth token cannot be empty".into());
            }
            settings.mesh.auth_token = Some(SmolStr::new(token));
            updated.push("mesh.auth_token");
            restart_required.push("mesh.auth_token");
        } else {
            return ControlResponse::error(id, "invalid mesh.auth_token".into());
        }
    }

    if let Some(value) = params.get("control.auth_token") {
        let mut auth_guard = match state.auth_token.lock() {
            Ok(guard) => guard,
            Err(_) => return ControlResponse::error(id, "auth token unavailable".into()),
        };
        if value.is_null() {
            if state.control_requires_auth {
                return ControlResponse::error(id, "auth token required for tcp endpoints".into());
            }
            *auth_guard = None;
            updated.push("control.auth_token");
        } else if let Some(token) = value.as_str() {
            if token.trim().is_empty() {
                return ControlResponse::error(id, "auth token cannot be empty".into());
            }
            *auth_guard = Some(SmolStr::new(token));
            updated.push("control.auth_token");
        } else {
            return ControlResponse::error(id, "invalid control.auth_token".into());
        }
    }
    if let Some(value) = params
        .get("control.debug_enabled")
        .and_then(|v| v.as_bool())
    {
        state.debug_enabled.store(value, Ordering::Relaxed);
        updated.push("control.debug_enabled");
    }
    if let Some(mode) = params.get("control.mode").and_then(|v| v.as_str()) {
        let parsed = match mode.trim().to_ascii_lowercase().as_str() {
            "production" => ControlMode::Production,
            "debug" => ControlMode::Debug,
            _ => {
                return ControlResponse::error(id, format!("invalid runtime.control.mode '{mode}'"))
            }
        };
        if let Ok(mut guard) = state.control_mode.lock() {
            *guard = parsed;
        }
        updated.push("control.mode");
        restart_required.push("control.mode");
    }

    let _ = state
        .resource
        .send_command(crate::scheduler::ResourceCommand::UpdateWatchdog(
            settings.watchdog,
        ));
    let _ = state
        .resource
        .send_command(crate::scheduler::ResourceCommand::UpdateFaultPolicy(
            settings.fault_policy,
        ));
    let _ =
        state
            .resource
            .send_command(crate::scheduler::ResourceCommand::UpdateRetainSaveInterval(
                settings.retain_save_interval,
            ));

    ControlResponse::ok(
        id,
        json!({ "updated": updated, "restart_required": restart_required }),
    )
}

fn handle_pause(id: u64, state: &ControlState) -> ControlResponse {
    let mode = state
        .control_mode
        .lock()
        .map(|value| *value)
        .unwrap_or(ControlMode::Production);
    if matches!(mode, ControlMode::Debug) {
        let _ = state
            .debug
            .apply_action(crate::debug::ControlAction::Pause(None));
    } else {
        let _ = state.resource.pause();
    }
    ControlResponse::ok(id, json!({"status": "paused"}))
}

fn handle_resume(id: u64, state: &ControlState) -> ControlResponse {
    let mode = state
        .control_mode
        .lock()
        .map(|value| *value)
        .unwrap_or(ControlMode::Production);
    if matches!(mode, ControlMode::Debug) {
        let _ = state
            .debug
            .apply_action(crate::debug::ControlAction::Continue);
    } else {
        let _ = state.resource.resume();
    }
    ControlResponse::ok(id, json!({"status": "running"}))
}

#[derive(Debug, Clone, Copy)]
enum StepKind {
    In,
    Over,
    Out,
}

fn handle_step(id: u64, state: &ControlState, kind: StepKind) -> ControlResponse {
    let action = match kind {
        StepKind::In => crate::debug::ControlAction::StepIn(None),
        StepKind::Over => crate::debug::ControlAction::StepOver(None),
        StepKind::Out => crate::debug::ControlAction::StepOut(None),
    };
    let _ = state.debug.apply_action(action);
    ControlResponse::ok(id, json!({"status": "stepping"}))
}

fn handle_debug_state(id: u64, state: &ControlState) -> ControlResponse {
    let paused = state.debug.is_paused();
    let last_stop = state
        .debug
        .last_stop()
        .and_then(|stop| debug_stop_to_json(stop, state));
    ControlResponse::ok(
        id,
        json!({
            "paused": paused,
            "last_stop": last_stop,
        }),
    )
}

fn handle_debug_stops(id: u64, state: &ControlState) -> ControlResponse {
    let stops = state
        .debug
        .drain_stops()
        .into_iter()
        .filter_map(|stop| debug_stop_to_json(stop, state))
        .collect::<Vec<_>>();
    ControlResponse::ok(id, json!({ "stops": stops }))
}

fn handle_debug_stack(id: u64, state: &ControlState) -> ControlResponse {
    let snapshot = match state.debug.snapshot() {
        Some(snapshot) => snapshot,
        None => return ControlResponse::error(id, "no snapshot available".into()),
    };
    let frames = snapshot.storage.frames();
    let frame_locations = state.debug.frame_locations();
    let fallback = state.debug.last_stop().and_then(|stop| stop.location);
    let mut stack_frames = Vec::new();
    if frames.is_empty() {
        if let Some(location) = fallback {
            if let Some(frame) = location_to_stack_frame(0, "Main", &location, state) {
                stack_frames.push(frame);
            }
        }
    } else {
        for frame in frames.iter().rev() {
            let resolved = frame_locations.get(&frame.id).copied().or(fallback);
            let frame_name = frame.owner.as_str();
            if let Some(location) = resolved {
                if let Some(frame_json) =
                    location_to_stack_frame(frame.id.0, frame_name, &location, state)
                {
                    stack_frames.push(frame_json);
                }
            }
        }
    }
    ControlResponse::ok(
        id,
        json!({
            "stack_frames": stack_frames,
            "total_frames": stack_frames.len(),
        }),
    )
}

fn handle_debug_scopes(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: DebugScopesParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    debug!("control debug.scopes frame_id={}", params.frame_id);
    let snapshot = match state.debug.snapshot() {
        Some(snapshot) => snapshot,
        None => return ControlResponse::error(id, "no snapshot available".into()),
    };
    let requested_frame = crate::memory::FrameId(params.frame_id);
    let current_frame = snapshot.storage.current_frame().map(|frame| frame.id);
    let has_requested_frame = snapshot
        .storage
        .frames()
        .iter()
        .any(|frame| frame.id == requested_frame);
    let frame_id = if has_requested_frame {
        requested_frame
    } else {
        current_frame.unwrap_or(requested_frame)
    };
    let location = state
        .debug
        .frame_location(frame_id)
        .or_else(|| state.debug.last_stop().and_then(|stop| stop.location))
        .and_then(|loc| location_to_source(&loc, state));
    let has_frame = has_requested_frame || current_frame.is_some();
    let (has_globals, has_retain, has_instances) = (
        !snapshot.storage.globals().is_empty(),
        !snapshot.storage.retain().is_empty(),
        !snapshot.storage.instances().is_empty(),
    );
    debug!(
        "control debug.scopes has_frame={} current_frame={:?} requested_frame={} has_globals={} has_retain={} has_instances={}",
        has_frame,
        current_frame,
        params.frame_id,
        has_globals,
        has_retain,
        has_instances
    );
    let io_snapshot = state
        .io_snapshot
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let has_io = crate::debug::dap::io_scope_available(io_snapshot.as_ref());

    let mut handles = match state.debug_variables.lock() {
        Ok(guard) => guard,
        Err(_) => return ControlResponse::error(id, "debug variables unavailable".into()),
    };
    handles.clear();

    let mut scopes = Vec::new();
    if has_frame {
        let locals_ref = handles.alloc(VariableHandle::Locals(frame_id));
        scopes.push(DebugScope {
            name: "Locals".to_string(),
            variables_reference: locals_ref,
            expensive: false,
            source: location.as_ref().map(|(source, _, _)| source.clone()),
            line: location.as_ref().map(|(_, line, _)| *line),
            column: location.as_ref().map(|(_, _, column)| *column),
            end_line: None,
            end_column: None,
        });
    }
    if has_globals {
        let globals_ref = handles.alloc(VariableHandle::Globals);
        scopes.push(DebugScope {
            name: "Globals".to_string(),
            variables_reference: globals_ref,
            expensive: false,
            source: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        });
    }
    if has_retain {
        let retain_ref = handles.alloc(VariableHandle::Retain);
        scopes.push(DebugScope {
            name: "Retain".to_string(),
            variables_reference: retain_ref,
            expensive: false,
            source: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        });
    }
    if has_io {
        let io_ref = handles.alloc(VariableHandle::IoRoot);
        scopes.push(DebugScope {
            name: "I/O".to_string(),
            variables_reference: io_ref,
            expensive: false,
            source: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        });
    }
    if has_instances {
        let instances_ref = handles.alloc(VariableHandle::Instances);
        scopes.push(DebugScope {
            name: "Instances".to_string(),
            variables_reference: instances_ref,
            expensive: false,
            source: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        });
    }

    debug!(
        "control debug.scopes result={:?}",
        scopes
            .iter()
            .map(|scope| scope.name.as_str())
            .collect::<Vec<_>>()
    );
    ControlResponse::ok(id, json!({ "scopes": scopes }))
}

fn handle_debug_variables(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: DebugVariablesParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    debug!(
        "control debug.variables reference={}",
        params.variables_reference
    );
    let snapshot = match state.debug.snapshot() {
        Some(snapshot) => snapshot,
        None => return ControlResponse::error(id, "no snapshot available".into()),
    };
    let io_snapshot = state
        .io_snapshot
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let mut handles = match state.debug_variables.lock() {
        Ok(guard) => guard,
        Err(_) => return ControlResponse::error(id, "debug variables unavailable".into()),
    };
    let Some(handle) = handles.get(params.variables_reference).cloned() else {
        return ControlResponse::ok(id, json!({ "variables": [] }));
    };
    debug!("control debug.variables handle={:?}", handle);
    let variables = match handle {
        VariableHandle::Locals(frame_id) => {
            let entries = snapshot
                .storage
                .frames()
                .iter()
                .find(|frame| frame.id == frame_id)
                .map(|frame| {
                    let mut entries = Vec::new();
                    if let Some(instance_id) = frame.instance_id {
                        if let Some(instance) = snapshot.storage.get_instance(instance_id) {
                            entries.extend(
                                instance
                                    .variables
                                    .iter()
                                    .map(|(name, value)| (name.to_string(), value.clone())),
                            );
                        }
                    }
                    entries.extend(
                        frame
                            .variables
                            .iter()
                            .map(|(name, value)| (name.to_string(), value.clone())),
                    );
                    entries
                })
                .unwrap_or_default();
            crate::debug::dap::variables_from_entries(&mut handles, entries)
        }
        VariableHandle::Globals => {
            let entries = snapshot
                .storage
                .globals()
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect::<Vec<_>>();
            crate::debug::dap::variables_from_entries(&mut handles, entries)
        }
        VariableHandle::Retain => {
            let entries = snapshot
                .storage
                .retain()
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect::<Vec<_>>();
            crate::debug::dap::variables_from_entries(&mut handles, entries)
        }
        VariableHandle::Instances => {
            let instances = snapshot
                .storage
                .instances()
                .iter()
                .map(|(id, data)| (*id, format!("{}#{}", data.type_name, id.0)))
                .collect::<Vec<_>>();
            crate::debug::dap::variables_from_instances(&mut handles, instances)
        }
        VariableHandle::Instance(instance_id) => {
            let entries = snapshot
                .storage
                .get_instance(instance_id)
                .map(|instance| {
                    let mut entries = instance
                        .variables
                        .iter()
                        .map(|(name, value)| (name.to_string(), value.clone()))
                        .collect::<Vec<_>>();
                    if let Some(parent) = instance.parent {
                        entries.push(("parent".to_string(), Value::Instance(parent)));
                    }
                    entries
                })
                .unwrap_or_default();
            crate::debug::dap::variables_from_entries(&mut handles, entries)
        }
        VariableHandle::Struct(value) => {
            crate::debug::dap::variables_from_struct(&mut handles, value)
        }
        VariableHandle::Array(value) => {
            crate::debug::dap::variables_from_array(&mut handles, value)
        }
        VariableHandle::Reference(value_ref) => {
            let value = snapshot.storage.read_by_ref(value_ref).cloned();
            value
                .map(|value| {
                    vec![crate::debug::dap::variable_from_value(
                        &mut handles,
                        "*".to_string(),
                        value,
                        None,
                    )]
                })
                .unwrap_or_default()
        }
        VariableHandle::IoRoot => {
            let inputs_ref = handles.alloc(VariableHandle::IoInputs);
            let outputs_ref = handles.alloc(VariableHandle::IoOutputs);
            let memory_ref = handles.alloc(VariableHandle::IoMemory);
            let state = io_snapshot.unwrap_or_default();
            vec![
                DebugVariable {
                    name: "Inputs".to_string(),
                    value: format!("{} items", state.inputs.len()),
                    r#type: None,
                    variables_reference: inputs_ref,
                    evaluate_name: None,
                },
                DebugVariable {
                    name: "Outputs".to_string(),
                    value: format!("{} items", state.outputs.len()),
                    r#type: None,
                    variables_reference: outputs_ref,
                    evaluate_name: None,
                },
                DebugVariable {
                    name: "Memory".to_string(),
                    value: format!("{} items", state.memory.len()),
                    r#type: None,
                    variables_reference: memory_ref,
                    evaluate_name: None,
                },
            ]
        }
        VariableHandle::IoInputs => {
            let state = io_snapshot.unwrap_or_default();
            crate::debug::dap::variables_from_io_entries(&state.inputs)
        }
        VariableHandle::IoOutputs => {
            let state = io_snapshot.unwrap_or_default();
            crate::debug::dap::variables_from_io_entries(&state.outputs)
        }
        VariableHandle::IoMemory => {
            let state = io_snapshot.unwrap_or_default();
            crate::debug::dap::variables_from_io_entries(&state.memory)
        }
    };
    debug!("control debug.variables result_count={}", variables.len());
    ControlResponse::ok(id, json!({ "variables": variables }))
}

fn handle_debug_evaluate(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: DebugEvaluateParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let snapshot = match state.debug.snapshot() {
        Some(snapshot) => snapshot,
        None => return ControlResponse::error(id, "no snapshot available".into()),
    };
    let frame_id = params.frame_id.map(crate::memory::FrameId);
    if let Some(frame_id) = frame_id {
        if !snapshot
            .storage
            .frames()
            .iter()
            .any(|frame| frame.id == frame_id)
        {
            return ControlResponse::error(id, "unknown frame id".into());
        }
    }
    let metadata = match state.metadata.lock() {
        Ok(guard) => guard,
        Err(_) => return ControlResponse::error(id, "metadata unavailable".into()),
    };
    let using = frame_id
        .and_then(|frame_id| metadata.using_for_frame(&snapshot.storage, frame_id))
        .unwrap_or_default();
    let mut registry = metadata.registry().clone();
    let expr = match crate::harness::parse_debug_expression(
        &params.expression,
        &mut registry,
        metadata.profile(),
        &using,
    ) {
        Ok(expr) => expr,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    let value = match evaluate_with_snapshot(&expr, &registry, frame_id, &snapshot, &using, state) {
        Ok(value) => value,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    let result = crate::debug::dap::format_value(&value);
    let type_name = crate::debug::dap::value_type_name(&value);
    ControlResponse::ok(
        id,
        json!({
            "result": result,
            "type": type_name,
            "variables_reference": 0,
        }),
    )
}

fn handle_debug_breakpoint_locations(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: DebugBreakpointLocationsParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let path = PathBuf::from(params.source);
    let file_id = match state.sources.file_id_for_path(&path) {
        Some(id) => id,
        None => return ControlResponse::error(id, "unknown source path".into()),
    };
    let source_text = match state.sources.source_text(file_id) {
        Some(text) => text,
        None => return ControlResponse::error(id, "source text not loaded".into()),
    };
    let metadata = match state.metadata.lock() {
        Ok(guard) => guard,
        Err(_) => return ControlResponse::error(id, "metadata unavailable".into()),
    };
    let mut breakpoints = Vec::new();
    if let Some(locations) = metadata.statement_locations(file_id) {
        let max_line = params.end_line.unwrap_or(params.line);
        for location in locations {
            let (loc_line, loc_col) = location_to_line_col(source_text, location);
            if loc_line < params.line || loc_line > max_line {
                continue;
            }
            if let Some(min_col) = params.column {
                if loc_line == params.line && loc_col < min_col {
                    continue;
                }
            }
            if let Some(max_col) = params.end_column {
                if loc_line == max_line && loc_col > max_col {
                    continue;
                }
            }
            breakpoints.push(json!({ "line": loc_line, "column": loc_col }));
        }
    }
    ControlResponse::ok(id, json!({ "breakpoints": breakpoints }))
}

fn debug_stop_to_json(
    stop: crate::debug::DebugStop,
    state: &ControlState,
) -> Option<serde_json::Value> {
    let reason = match stop.reason {
        crate::debug::DebugStopReason::Breakpoint => "breakpoint",
        crate::debug::DebugStopReason::Step => "step",
        crate::debug::DebugStopReason::Pause => "pause",
        crate::debug::DebugStopReason::Entry => "entry",
    };
    let mut payload = json!({
        "reason": reason,
        "thread_id": stop.thread_id,
        "breakpoint_generation": stop.breakpoint_generation,
    });
    if let Some(location) = stop.location {
        if let Some(text) = state.sources.source_text(location.file_id) {
            let (line, column) = location_to_line_col(text, &location);
            if let Some(source) = state
                .sources
                .files()
                .iter()
                .find(|file| file.id == location.file_id)
            {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("file_id".to_string(), json!(location.file_id));
                    obj.insert("line".to_string(), json!(line));
                    obj.insert("column".to_string(), json!(column));
                    obj.insert(
                        "path".to_string(),
                        json!(source.path.to_string_lossy().to_string()),
                    );
                }
            }
        }
    }
    Some(payload)
}

fn location_to_source(
    location: &crate::debug::SourceLocation,
    state: &ControlState,
) -> Option<(DebugSource, u32, u32)> {
    let source_text = state.sources.source_text(location.file_id)?;
    let (line, column) = location_to_line_col(source_text, location);
    let source = state
        .sources
        .files()
        .iter()
        .find(|file| file.id == location.file_id)?;
    let name = source
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string());
    let path = Some(source.path.to_string_lossy().to_string());
    Some((DebugSource { name, path }, line, column))
}

fn location_to_stack_frame(
    frame_id: u32,
    frame_name: &str,
    location: &crate::debug::SourceLocation,
    state: &ControlState,
) -> Option<serde_json::Value> {
    let (source, line, column) = location_to_source(location, state)?;
    Some(json!({
        "id": frame_id,
        "name": frame_name,
        "source": source,
        "line": line,
        "column": column,
    }))
}

fn evaluate_with_snapshot(
    expr: &crate::eval::expr::Expr,
    registry: &trust_hir::types::TypeRegistry,
    frame_id: Option<crate::memory::FrameId>,
    snapshot: &crate::debug::DebugSnapshot,
    using: &[smol_str::SmolStr],
    state: &ControlState,
) -> Result<Value, RuntimeError> {
    let metadata = state
        .metadata
        .lock()
        .map_err(|_| RuntimeError::ControlError("metadata unavailable".into()))?;
    let profile = metadata.profile();
    let now = snapshot.now;
    let functions = metadata.functions();
    let stdlib = metadata.stdlib();
    let function_blocks = metadata.function_blocks();
    let classes = metadata.classes();
    let access = metadata.access_map();

    let mut storage = snapshot.storage.clone();
    let eval = |storage: &mut crate::memory::VariableStorage,
                instance_id: Option<crate::memory::InstanceId>|
     -> Result<Value, RuntimeError> {
        let mut ctx = crate::eval::EvalContext {
            storage,
            registry,
            profile,
            now,
            debug: None,
            call_depth: 0,
            functions: Some(functions),
            stdlib: Some(stdlib),
            function_blocks: Some(function_blocks),
            classes: Some(classes),
            using: if using.is_empty() { None } else { Some(using) },
            access: Some(access),
            current_instance: instance_id,
            return_name: None,
            loop_depth: 0,
            pause_requested: false,
        };
        crate::eval::eval_expr(&mut ctx, expr)
    };

    if let Some(frame_id) = frame_id {
        storage
            .with_frame(frame_id, |storage| {
                let instance_id = storage.current_frame().and_then(|frame| frame.instance_id);
                eval(storage, instance_id)
            })
            .ok_or(RuntimeError::InvalidFrame(frame_id.0))?
    } else {
        eval(&mut storage, None)
    }
}

fn handle_breakpoints_set(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    if state.sources.is_empty() {
        return ControlResponse::error(id, "no sources registered".into());
    }
    let params: BreakpointsParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let path = PathBuf::from(params.source);
    let file_id = match state.sources.file_id_for_path(&path) {
        Some(id) => id,
        None => return ControlResponse::error(id, "unknown source path".into()),
    };
    let source_text = match state.sources.source_text(file_id) {
        Some(text) => text,
        None => return ControlResponse::error(id, "source text not loaded".into()),
    };
    let metadata = match state.metadata.lock() {
        Ok(guard) => guard,
        Err(_) => return ControlResponse::error(id, "metadata unavailable".into()),
    };
    let mut breakpoints = Vec::new();
    let mut resolved = Vec::new();
    for line in params.lines {
        if let Some((location, resolved_line, resolved_col)) =
            metadata.resolve_breakpoint_position(source_text, file_id, line, 1)
        {
            breakpoints.push(DebugBreakpoint::new(location));
            resolved.push(json!({"line": resolved_line, "column": resolved_col}));
        }
    }
    state.debug.set_breakpoints_for_file(file_id, breakpoints);
    let generation = state.debug.breakpoint_generation(file_id);
    ControlResponse::ok(
        id,
        json!({
            "status": "ok",
            "file_id": file_id,
            "resolved": resolved,
            "generation": generation,
        }),
    )
}

fn handle_breakpoints_clear(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: BreakpointsParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let path = PathBuf::from(params.source);
    let file_id = match state.sources.file_id_for_path(&path) {
        Some(id) => id,
        None => return ControlResponse::error(id, "unknown source path".into()),
    };
    state.debug.set_breakpoints_for_file(file_id, Vec::new());
    ControlResponse::ok(id, json!({"status": "cleared"}))
}

fn handle_breakpoints_list(id: u64, state: &ControlState) -> ControlResponse {
    let breakpoints = state
        .debug
        .breakpoints()
        .into_iter()
        .map(|bp| {
            json!({
                "file_id": bp.location.file_id,
                "start": bp.location.start,
                "end": bp.location.end,
            })
        })
        .collect::<Vec<_>>();
    ControlResponse::ok(id, json!({ "breakpoints": breakpoints }))
}

fn handle_breakpoints_clear_all(id: u64, state: &ControlState) -> ControlResponse {
    state.debug.clear_breakpoints();
    ControlResponse::ok(id, json!({ "status": "cleared" }))
}

fn handle_breakpoints_clear_id(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: BreakpointsClearIdParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    if state.sources.source_text(params.file_id).is_none() {
        return ControlResponse::error(id, "unknown file id".into());
    }
    state
        .debug
        .set_breakpoints_for_file(params.file_id, Vec::new());
    ControlResponse::ok(
        id,
        json!({ "status": "cleared", "file_id": params.file_id }),
    )
}

fn handle_io_read(id: u64, state: &ControlState) -> ControlResponse {
    let snapshot = state
        .io_snapshot
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    ControlResponse::ok(
        id,
        json!({
            "snapshot": snapshot.map(|snap| snap.into_json())
        }),
    )
}

fn handle_io_write(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: IoWriteParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let address = match IoAddress::parse(&params.address) {
        Ok(addr) => addr,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    let value = match parse_value(&params.value) {
        Ok(value) => value,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    state.debug.enqueue_io_write(address, value);
    ControlResponse::ok(id, json!({"status": "queued"}))
}

fn handle_io_force(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: IoWriteParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let address = match IoAddress::parse(&params.address) {
        Ok(addr) => addr,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    let value = match parse_value(&params.value) {
        Ok(value) => value,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    state.debug.force_io(address, value);
    ControlResponse::ok(id, json!({"status": "forced"}))
}

fn handle_io_unforce(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: IoAddressParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let address = match IoAddress::parse(&params.address) {
        Ok(addr) => addr,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    state.debug.release_io(&address);
    ControlResponse::ok(id, json!({"status": "released"}))
}

fn handle_eval(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: EvalParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let snapshot = match state.debug.snapshot() {
        Some(snapshot) => snapshot,
        None => return ControlResponse::error(id, "no snapshot available".into()),
    };
    let name = params.expr.trim();
    let value = snapshot
        .storage
        .get_global(name)
        .cloned()
        .or_else(|| snapshot.storage.get_retain(name).cloned());
    match value {
        Some(value) => ControlResponse::ok(id, json!({ "value": format!("{value:?}") })),
        None => ControlResponse::error(id, "unknown identifier".into()),
    }
}

fn handle_set(id: u64, params: Option<serde_json::Value>, state: &ControlState) -> ControlResponse {
    let params: SetParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let value = match parse_value(&params.value) {
        Ok(value) => value,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    if let Some(name) = params.target.strip_prefix("global:") {
        state.debug.enqueue_global_write(name.trim(), value);
        return ControlResponse::ok(id, json!({"status": "queued"}));
    }
    if let Some(name) = params.target.strip_prefix("retain:") {
        state.debug.enqueue_retain_write(name.trim(), value);
        return ControlResponse::ok(id, json!({"status": "queued"}));
    }
    ControlResponse::error(id, "unsupported target".into())
}

fn parse_var_target(target: &str) -> Result<VarTarget, String> {
    if let Some(name) = target.strip_prefix("global:") {
        if name.trim().is_empty() {
            return Err("missing global name".into());
        }
        return Ok(VarTarget::Global(name.trim().to_string()));
    }
    if let Some(name) = target.strip_prefix("retain:") {
        if name.trim().is_empty() {
            return Err("missing retain name".into());
        }
        return Ok(VarTarget::Retain(name.trim().to_string()));
    }
    if let Some(rest) = target.strip_prefix("instance:") {
        let mut parts = rest.splitn(2, ':');
        let id = parts
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| "invalid instance id".to_string())?;
        let name = parts.next().unwrap_or("").trim();
        if name.is_empty() {
            return Err("missing instance name".into());
        }
        return Ok(VarTarget::Instance(id, name.to_string()));
    }
    Err("unsupported target (use global:<name> or retain:<name>)".into())
}

fn handle_var_force(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: VarForceParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let target = match parse_var_target(&params.target) {
        Ok(target) => target,
        Err(err) => return ControlResponse::error(id, err),
    };
    let value = match parse_value(&params.value) {
        Ok(value) => value,
        Err(err) => return ControlResponse::error(id, err.to_string()),
    };
    match target {
        VarTarget::Global(name) => state.debug.force_global(name, value),
        VarTarget::Retain(name) => state.debug.force_retain(name, value),
        VarTarget::Instance(id, name) => {
            state
                .debug
                .force_instance(crate::memory::InstanceId(id), name, value)
        }
    }
    ControlResponse::ok(id, json!({ "status": "forced" }))
}

fn handle_var_unforce(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: VarTargetParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let target = match parse_var_target(&params.target) {
        Ok(target) => target,
        Err(err) => return ControlResponse::error(id, err),
    };
    match target {
        VarTarget::Global(name) => state.debug.release_global(&name),
        VarTarget::Retain(name) => state.debug.release_retain(&name),
        VarTarget::Instance(id, name) => state
            .debug
            .release_instance(crate::memory::InstanceId(id), &name),
    }
    ControlResponse::ok(id, json!({ "status": "released" }))
}

fn handle_var_forced(id: u64, state: &ControlState) -> ControlResponse {
    let snapshot = state.debug.forced_snapshot();
    let vars = snapshot
        .vars
        .into_iter()
        .map(|entry| {
            let target = match entry.target {
                crate::debug::ForcedVarTarget::Global(name) => {
                    format!("global:{name}")
                }
                crate::debug::ForcedVarTarget::Retain(name) => {
                    format!("retain:{name}")
                }
                crate::debug::ForcedVarTarget::Instance(id, name) => {
                    format!("instance:{}:{name}", id.0)
                }
            };
            json!({
                "target": target,
                "value": crate::debug::dap::format_value(&entry.value),
            })
        })
        .collect::<Vec<_>>();
    ControlResponse::ok(id, json!({ "vars": vars }))
}

fn handle_shutdown(id: u64, state: &ControlState) -> ControlResponse {
    state.resource.stop();
    ControlResponse::ok(id, json!({"status": "stopping"}))
}

fn handle_restart(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: RestartParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let mode = match params.mode.to_ascii_lowercase().as_str() {
        "cold" => RestartMode::Cold,
        "warm" => RestartMode::Warm,
        _ => return ControlResponse::error(id, "invalid restart mode".into()),
    };
    if let Ok(mut guard) = state.pending_restart.lock() {
        *guard = Some(mode);
    }
    ControlResponse::ok(id, json!({"status": "restart queued"}))
}

fn handle_bytecode_reload(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: BytecodeReloadParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let bytes = match BASE64_STANDARD.decode(params.bytes.as_bytes()) {
        Ok(bytes) => bytes,
        Err(err) => return ControlResponse::error(id, format!("invalid bytecode: {err}")),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    if let Err(err) = state
        .resource
        .send_command(ResourceCommand::ReloadBytecode {
            bytes,
            respond_to: tx,
        })
    {
        return ControlResponse::error(id, err.to_string());
    }
    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(metadata)) => {
            if let Ok(mut guard) = state.metadata.lock() {
                *guard = metadata;
            }
            ControlResponse::ok(id, json!({ "status": "reloaded" }))
        }
        Ok(Err(err)) => ControlResponse::error(id, err.to_string()),
        Err(_) => ControlResponse::error(id, "reload timeout".into()),
    }
}

fn handle_pair_start(id: u64, state: &ControlState) -> ControlResponse {
    let Some(store) = state.pairing.as_ref() else {
        return ControlResponse::error(id, "pairing unavailable".into());
    };
    let code = store.start_pairing();
    ControlResponse::ok(
        id,
        json!({ "code": code.code, "expires_at": code.expires_at }),
    )
}

fn handle_pair_claim(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: PairClaimParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let Some(store) = state.pairing.as_ref() else {
        return ControlResponse::error(id, "pairing unavailable".into());
    };
    match store.claim(&params.code) {
        Some(token) => ControlResponse::ok(id, json!({ "token": token })),
        None => ControlResponse::error(id, "invalid or expired code".into()),
    }
}

fn handle_pair_list(id: u64, state: &ControlState) -> ControlResponse {
    let Some(store) = state.pairing.as_ref() else {
        return ControlResponse::error(id, "pairing unavailable".into());
    };
    let tokens = store.list();
    ControlResponse::ok(id, json!({ "tokens": tokens }))
}

fn handle_pair_revoke(
    id: u64,
    params: Option<serde_json::Value>,
    state: &ControlState,
) -> ControlResponse {
    let params: PairRevokeParams = match params {
        Some(value) => match serde_json::from_value(value) {
            Ok(parsed) => parsed,
            Err(err) => return ControlResponse::error(id, format!("invalid params: {err}")),
        },
        None => return ControlResponse::error(id, "missing params".into()),
    };
    let Some(store) = state.pairing.as_ref() else {
        return ControlResponse::error(id, "pairing unavailable".into());
    };
    if params.id == "all" {
        let count = store.revoke_all();
        return ControlResponse::ok(id, json!({ "status": "revoked", "count": count }));
    }
    if store.revoke(&params.id) {
        ControlResponse::ok(id, json!({ "status": "revoked", "id": params.id }))
    } else {
        ControlResponse::error(id, "unknown token id".into())
    }
}

fn parse_value(text: &str) -> Result<Value, RuntimeError> {
    let upper = text.trim().to_ascii_uppercase();
    if upper == "TRUE" {
        return Ok(Value::Bool(true));
    }
    if upper == "FALSE" {
        return Ok(Value::Bool(false));
    }
    if let Ok(int_val) = upper.parse::<i64>() {
        return Ok(Value::LInt(int_val));
    }
    Err(RuntimeError::ControlError(
        format!("unsupported value '{text}'").into(),
    ))
}

#[derive(Debug, Deserialize)]
struct ControlRequest {
    id: u64,
    #[serde(rename = "type")]
    r#type: String,
    params: Option<serde_json::Value>,
    auth: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ControlResponse {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl ControlResponse {
    fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: u64, error: String) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BreakpointsParams {
    source: String,
    lines: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct BreakpointsClearIdParams {
    file_id: u32,
}

#[derive(Debug, Deserialize)]
struct DebugScopesParams {
    frame_id: u32,
}

#[derive(Debug, Deserialize)]
struct DebugVariablesParams {
    variables_reference: u32,
}

#[derive(Debug, Deserialize)]
struct DebugEvaluateParams {
    expression: String,
    frame_id: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DebugBreakpointLocationsParams {
    source: String,
    line: u32,
    end_line: Option<u32>,
    column: Option<u32>,
    end_column: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct IoWriteParams {
    address: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct IoAddressParams {
    address: String,
}

#[derive(Debug, Deserialize)]
struct RestartParams {
    mode: String,
}

#[derive(Debug, Deserialize)]
struct BytecodeReloadParams {
    bytes: String,
}

#[derive(Debug, Deserialize)]
struct EvalParams {
    expr: String,
}

#[derive(Debug, Deserialize)]
struct SetParams {
    target: String,
    value: String,
}

enum VarTarget {
    Global(String),
    Retain(String),
    Instance(u32, String),
}

#[derive(Deserialize)]
struct VarForceParams {
    target: String,
    value: String,
}

#[derive(Deserialize)]
struct VarTargetParams {
    target: String,
}

#[derive(Debug, Deserialize)]
struct PairClaimParams {
    code: String,
}

#[derive(Debug, Deserialize)]
struct PairRevokeParams {
    id: String,
}

trait IoSnapshotJson {
    fn into_json(self) -> serde_json::Value;
}

impl IoSnapshotJson for IoSnapshot {
    fn into_json(self) -> serde_json::Value {
        json!({
            "inputs": self.inputs.iter().map(entry_to_json).collect::<Vec<_>>(),
            "outputs": self.outputs.iter().map(entry_to_json).collect::<Vec<_>>(),
            "memory": self.memory.iter().map(entry_to_json).collect::<Vec<_>>(),
        })
    }
}

fn entry_to_json(entry: &crate::io::IoSnapshotEntry) -> serde_json::Value {
    json!({
        "name": entry.name.as_ref().map(|name| name.as_str()),
        "address": format_address(&entry.address),
        "value": format_snapshot_value(&entry.value),
    })
}

fn format_snapshot_value(value: &crate::io::IoSnapshotValue) -> serde_json::Value {
    match value {
        crate::io::IoSnapshotValue::Value(value) => json!(format!("{value:?}")),
        crate::io::IoSnapshotValue::Error(err) => json!({ "error": err }),
        crate::io::IoSnapshotValue::Unresolved => json!("unresolved"),
    }
}

fn format_address(address: &IoAddress) -> String {
    let area = match address.area {
        crate::memory::IoArea::Input => "I",
        crate::memory::IoArea::Output => "Q",
        crate::memory::IoArea::Memory => "M",
    };
    let size = match address.size {
        crate::io::IoSize::Bit => "X",
        crate::io::IoSize::Byte => "B",
        crate::io::IoSize::Word => "W",
        crate::io::IoSize::DWord => "D",
        crate::io::IoSize::LWord => "L",
    };
    if address.wildcard {
        return format!("%{area}{size}*");
    }
    if address.size == crate::io::IoSize::Bit {
        format!("%{area}{size}{}.{}", address.byte, address.bit)
    } else {
        format!("%{area}{size}{}", address.byte)
    }
}

fn runtime_event_to_json(event: crate::debug::RuntimeEvent) -> serde_json::Value {
    match event {
        crate::debug::RuntimeEvent::CycleStart { cycle, time } => json!({
            "type": "cycle_start",
            "cycle": cycle,
            "time_ns": time.as_nanos(),
        }),
        crate::debug::RuntimeEvent::CycleEnd { cycle, time } => json!({
            "type": "cycle_end",
            "cycle": cycle,
            "time_ns": time.as_nanos(),
        }),
        crate::debug::RuntimeEvent::TaskStart {
            name,
            priority,
            time,
        } => json!({
            "type": "task_start",
            "name": name.as_str(),
            "priority": priority,
            "time_ns": time.as_nanos(),
        }),
        crate::debug::RuntimeEvent::TaskEnd {
            name,
            priority,
            time,
        } => json!({
            "type": "task_end",
            "name": name.as_str(),
            "priority": priority,
            "time_ns": time.as_nanos(),
        }),
        crate::debug::RuntimeEvent::TaskOverrun { name, missed, time } => json!({
            "type": "task_overrun",
            "name": name.as_str(),
            "missed": missed,
            "time_ns": time.as_nanos(),
        }),
        crate::debug::RuntimeEvent::Fault { error, time } => json!({
            "type": "fault",
            "error": error,
            "time_ns": time.as_nanos(),
        }),
    }
}

fn io_health_to_json(entry: &IoDriverStatus) -> serde_json::Value {
    match &entry.health {
        IoDriverHealth::Ok => json!({
            "name": entry.name.as_str(),
            "status": "ok",
        }),
        IoDriverHealth::Degraded { error } => json!({
            "name": entry.name.as_str(),
            "status": "degraded",
            "error": error.as_str(),
        }),
        IoDriverHealth::Faulted { error } => json!({
            "name": entry.name.as_str(),
            "status": "faulted",
            "error": error.as_str(),
        }),
    }
}
