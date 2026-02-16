//! Embedded browser UI server.

#![allow(missing_docs)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use qrcode::{render::svg, QrCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use smol_str::SmolStr;
use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::bundle_template::{IoConfigTemplate, IoDriverTemplate};
use crate::config::{
    load_system_io_config, IoConfig, IoDriverConfig, RuntimeConfig, WebAuthMode, WebConfig,
};
use crate::control::{handle_request_value, ControlState};
use crate::debug::dap::format_value;
use crate::discovery::DiscoveryState;
use crate::error::RuntimeError;
use crate::io::{IoAddress, IoDriverRegistry, IoSize};
use crate::memory::IoArea;
use crate::security::{AccessRole, TlsMaterials};
use crate::setup::SetupOptions;

mod deploy;
pub mod ide;
pub mod pairing;

use deploy::{apply_deploy, apply_rollback, DeployRequest};
use ide::{IdeError, IdeRole, WebIdeFrontendTelemetry, WebIdeState};
use pairing::PairingStore;

#[derive(Debug, Deserialize)]
struct SetupApplyRequest {
    #[serde(alias = "bundle_path")]
    project_path: Option<String>,
    resource_name: Option<String>,
    cycle_ms: Option<u64>,
    driver: Option<String>,
    write_system_io: Option<bool>,
    overwrite_system_io: Option<bool>,
    use_system_io: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RollbackRequest {
    restart: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IoConfigRequest {
    driver: Option<String>,
    params: Option<serde_json::Value>,
    drivers: Option<Vec<IoDriverConfigRequest>>,
    safe_state: Option<Vec<IoSafeStateEntry>>,
    use_system_io: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct IoDriverConfigRequest {
    name: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct IoConfigResponse {
    driver: String,
    params: serde_json::Value,
    drivers: Vec<IoDriverConfigResponse>,
    safe_state: Vec<IoSafeStateEntry>,
    supported_drivers: Vec<String>,
    source: String,
    use_system_io: bool,
}

#[derive(Debug, Clone, Serialize)]
struct IoDriverConfigResponse {
    name: String,
    params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct IdeSessionRequest {
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdeProjectOpenRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
struct IdeWriteRequest {
    path: String,
    expected_version: u64,
    content: String,
}

#[derive(Debug, Deserialize)]
struct IdeFsCreateRequest {
    path: String,
    kind: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdeFsRenameRequest {
    path: String,
    new_path: String,
}

#[derive(Debug, Deserialize)]
struct IdeFsDeleteRequest {
    path: String,
}

#[derive(Debug, Deserialize)]
struct IdeDiagnosticsRequest {
    path: String,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdeFormatRequest {
    path: String,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdePositionRequest {
    line: u32,
    character: u32,
}

#[derive(Debug, Deserialize)]
struct IdeHoverRequest {
    path: String,
    content: Option<String>,
    position: IdePositionRequest,
}

#[derive(Debug, Deserialize)]
struct IdeCompletionRequest {
    path: String,
    content: Option<String>,
    position: IdePositionRequest,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct IdeReferencesRequest {
    path: String,
    content: Option<String>,
    position: IdePositionRequest,
    include_declaration: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct IdeRenameRequest {
    path: String,
    content: Option<String>,
    position: IdePositionRequest,
    new_name: String,
}

#[derive(Debug, Deserialize)]
struct IdeFrontendTelemetryRequest {
    bootstrap_failures: Option<u64>,
    analysis_timeouts: Option<u64>,
    worker_restarts: Option<u64>,
    autosave_failures: Option<u64>,
}

#[derive(Debug, Clone)]
struct IdeTaskJob {
    job_id: u64,
    kind: String,
    status: String,
    success: Option<bool>,
    output: String,
    started_ms: u64,
    finished_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct IdeTaskLocation {
    path: String,
    line: u32,
    column: u32,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct IdeTaskSnapshot {
    job_id: u64,
    kind: String,
    status: String,
    success: Option<bool>,
    output: String,
    locations: Vec<IdeTaskLocation>,
    started_ms: u64,
    finished_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IoSafeStateEntry {
    address: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct SetupDefaultsResponse {
    project_path: String,
    resource_name: String,
    cycle_ms: u64,
    driver: String,
    supported_drivers: Vec<String>,
    use_system_io: bool,
    system_io_exists: bool,
    write_system_io: bool,
    needs_setup: bool,
}

const INDEX_HTML: &str = include_str!("web/ui/index.html");
const APP_JS: &str = include_str!("web/ui/app.js");
const APP_CSS: &str = include_str!("web/ui/styles.css");
const HMI_HTML: &str = include_str!("web/ui/hmi.html");
const HMI_JS: &str = include_str!("web/ui/hmi.js");
const HMI_CSS: &str = include_str!("web/ui/hmi.css");
const IDE_HTML: &str = include_str!("web/ui/ide.html");
const IDE_CSS: &str = include_str!("web/ui/ide.css");
const IDE_JS: &str = include_str!("web/ui/ide.js");
const IDE_MONACO_BUNDLE_JS: &str = include_str!("web/ui/assets/ide-monaco.20260215.js");
const IDE_MONACO_BUNDLE_CSS: &str = include_str!("web/ui/assets/ide-monaco.20260215.css");
const IDE_LOGO_SVG: &str = include_str!("web/ui/assets/logo.svg");
const IDE_WASM_WORKER_JS: &str = include_str!("web/ui/wasm/worker.js");
const IDE_WASM_CLIENT_JS: &str = include_str!("web/ui/wasm/analysis-client.js");
const HMI_WS_ROUTE: &str = "/ws/hmi";
const HMI_WS_VALUES_POLL_INTERVAL: Duration = Duration::from_millis(100);
const HMI_WS_SCHEMA_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HMI_WS_ALARMS_POLL_INTERVAL: Duration = Duration::from_millis(500);

fn default_bundle_root(bundle_root: &Option<PathBuf>) -> PathBuf {
    bundle_root
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_resource_name(bundle_root: &Path) -> SmolStr {
    let project_name = bundle_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("trust-plc");
    SmolStr::new(project_name.replace(|c: char| !c.is_ascii_alphanumeric(), "_"))
}

fn detect_default_driver() -> String {
    if crate::setup::is_raspberry_pi_hint() {
        "gpio".to_string()
    } else {
        "loopback".to_string()
    }
}

fn setup_defaults(bundle_root: &Option<PathBuf>) -> SetupDefaultsResponse {
    let project_path = default_bundle_root(bundle_root);
    let runtime_path = project_path.join("runtime.toml");
    let io_path = project_path.join("io.toml");

    let runtime_loaded = if runtime_path.exists() {
        RuntimeConfig::load(&runtime_path).ok()
    } else {
        None
    };
    let (resource_name, cycle_ms) = if let Some(runtime) = runtime_loaded.as_ref() {
        (
            runtime.resource_name.to_string(),
            runtime.cycle_interval.as_millis() as u64,
        )
    } else {
        (default_resource_name(&project_path).to_string(), 100)
    };

    let system_io = load_system_io_config().ok().flatten();
    let system_io_exists = system_io.is_some();

    let (driver, use_system_io) = if io_path.exists() {
        match IoConfig::load(&io_path) {
            Ok(io) => (
                io.drivers
                    .first()
                    .map(|driver| driver.name.to_string())
                    .unwrap_or_else(detect_default_driver),
                false,
            ),
            Err(_) => (detect_default_driver(), system_io_exists),
        }
    } else if let Some(system_io) = system_io {
        (
            system_io
                .drivers
                .first()
                .map(|driver| driver.name.to_string())
                .unwrap_or_else(detect_default_driver),
            true,
        )
    } else {
        (detect_default_driver(), false)
    };

    let write_system_io = !system_io_exists;
    let needs_setup = runtime_loaded.is_none() || (!io_path.exists() && !system_io_exists);

    SetupDefaultsResponse {
        project_path: project_path.display().to_string(),
        resource_name,
        cycle_ms,
        driver,
        supported_drivers: IoDriverRegistry::default_registry().canonical_driver_names(),
        use_system_io,
        system_io_exists,
        write_system_io,
        needs_setup,
    }
}

fn json_to_toml(value: &serde_json::Value) -> toml::Value {
    match value {
        serde_json::Value::Null => toml::Value::String(String::new()),
        serde_json::Value::Bool(value) => toml::Value::Boolean(*value),
        serde_json::Value::Number(value) => {
            if let Some(i) = value.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(u) = value.as_u64() {
                toml::Value::Integer(u as i64)
            } else if let Some(f) = value.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(value.to_string())
            }
        }
        serde_json::Value::String(value) => toml::Value::String(value.clone()),
        serde_json::Value::Array(values) => {
            toml::Value::Array(values.iter().map(json_to_toml).collect())
        }
        serde_json::Value::Object(values) => {
            let mut table = toml::map::Map::new();
            for (key, value) in values {
                table.insert(key.clone(), json_to_toml(value));
            }
            toml::Value::Table(table)
        }
    }
}

fn io_config_to_response(config: IoConfig, source: &str, use_system_io: bool) -> IoConfigResponse {
    let drivers = config
        .drivers
        .iter()
        .map(|driver| IoDriverConfigResponse {
            name: driver.name.to_string(),
            params: serde_json::to_value(&driver.params).unwrap_or_else(|_| json!({})),
        })
        .collect::<Vec<_>>();
    let primary = drivers.first().cloned().unwrap_or(IoDriverConfigResponse {
        name: detect_default_driver(),
        params: json!({}),
    });
    let safe_state = config
        .safe_state
        .outputs
        .iter()
        .map(|(address, value)| IoSafeStateEntry {
            address: format_io_address(address),
            value: format_value(value),
        })
        .collect::<Vec<_>>();
    IoConfigResponse {
        driver: primary.name,
        params: primary.params,
        drivers,
        safe_state,
        supported_drivers: IoDriverRegistry::default_registry().canonical_driver_names(),
        source: source.to_string(),
        use_system_io,
    }
}

fn load_io_config(bundle_root: &Option<PathBuf>) -> Result<IoConfigResponse, RuntimeError> {
    let project_root = default_bundle_root(bundle_root);
    let project_io = project_root.join("io.toml");
    if project_io.is_file() {
        let config = IoConfig::load(&project_io)?;
        return Ok(io_config_to_response(config, "project", false));
    }
    if let Some(system) = load_system_io_config().ok().flatten() {
        return Ok(io_config_to_response(system, "system", true));
    }
    Ok(IoConfigResponse {
        driver: detect_default_driver(),
        params: json!({}),
        drivers: vec![IoDriverConfigResponse {
            name: detect_default_driver(),
            params: json!({}),
        }],
        safe_state: Vec::new(),
        supported_drivers: IoDriverRegistry::default_registry().canonical_driver_names(),
        source: "default".to_string(),
        use_system_io: false,
    })
}

fn render_io_toml(drivers: Vec<IoDriverConfig>, safe_state: Vec<IoSafeStateEntry>) -> String {
    let template = IoConfigTemplate {
        drivers: drivers
            .into_iter()
            .map(|driver| IoDriverTemplate {
                name: driver.name.to_string(),
                params: driver.params,
            })
            .collect(),
        safe_state: safe_state
            .into_iter()
            .map(|entry| (entry.address, entry.value))
            .collect(),
    };
    crate::bundle_template::render_io_toml(&template)
}

fn driver_configs_from_payload(
    payload: &IoConfigRequest,
) -> Result<Vec<IoDriverConfig>, RuntimeError> {
    if let Some(drivers) = payload.drivers.as_ref() {
        if drivers.is_empty() {
            return Err(RuntimeError::InvalidConfig(
                "io.drivers must contain at least one driver".into(),
            ));
        }
        return drivers
            .iter()
            .enumerate()
            .map(|(idx, driver)| {
                let name = driver.name.trim();
                if name.is_empty() {
                    return Err(RuntimeError::InvalidConfig(
                        format!("io.drivers[{idx}].name must not be empty").into(),
                    ));
                }
                let params_json = driver.params.clone().unwrap_or_else(|| json!({}));
                let params_toml = json_to_toml(&params_json);
                if !params_toml.is_table() {
                    return Err(RuntimeError::InvalidConfig(
                        format!("io.drivers[{idx}].params must be a table/object").into(),
                    ));
                }
                Ok(IoDriverConfig {
                    name: SmolStr::new(name),
                    params: params_toml,
                })
            })
            .collect::<Result<Vec<_>, _>>();
    }

    let driver = payload
        .driver
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| RuntimeError::InvalidConfig("driver is required".into()))?;
    let params_json = payload.params.clone().unwrap_or_else(|| json!({}));
    let params_toml = json_to_toml(&params_json);
    if !params_toml.is_table() {
        return Err(RuntimeError::InvalidConfig(
            "params must be a table/object".into(),
        ));
    }
    Ok(vec![IoDriverConfig {
        name: SmolStr::new(driver),
        params: params_toml,
    }])
}

fn format_io_address(address: &IoAddress) -> String {
    let area = match address.area {
        IoArea::Input => "I",
        IoArea::Output => "Q",
        IoArea::Memory => "M",
    };
    let size = match address.size {
        IoSize::Bit => "X",
        IoSize::Byte => "B",
        IoSize::Word => "W",
        IoSize::DWord => "D",
        IoSize::LWord => "L",
    };
    if address.wildcard {
        return format!("%{}*", area);
    }
    if matches!(address.size, IoSize::Bit) {
        format!("%{}{}{}.{}", area, size, address.byte, address.bit)
    } else {
        format!("%{}{}{}", area, size, address.byte)
    }
}

fn list_sources(bundle_root: &Path) -> Vec<String> {
    let sources_dir = bundle_root.join("src");
    let mut list = Vec::new();
    let Ok(entries) = std::fs::read_dir(&sources_dir) else {
        return list;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("st") {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|v| v.to_str()) {
            list.push(name.to_string());
        }
    }
    list.sort();
    list
}

fn read_source_file(bundle_root: &Path, name: &str) -> Result<String, RuntimeError> {
    let sources_dir = bundle_root.join("src");
    let requested = sources_dir.join(name);
    let sources_dir = sources_dir
        .canonicalize()
        .map_err(|err| RuntimeError::InvalidConfig(format!("src dir unavailable: {err}").into()))?;
    let requested = requested
        .canonicalize()
        .map_err(|err| RuntimeError::InvalidConfig(format!("source not found: {err}").into()))?;
    if !requested.starts_with(&sources_dir) {
        return Err(RuntimeError::InvalidConfig("invalid source path".into()));
    }
    std::fs::read_to_string(&requested)
        .map_err(|err| RuntimeError::InvalidConfig(format!("failed to read source: {err}").into()))
}

fn read_hmi_asset_file(project_root: &Path, name: &str) -> Result<String, RuntimeError> {
    let hmi_dir = project_root.join("hmi");
    let requested = hmi_dir.join(name);
    let hmi_dir = hmi_dir
        .canonicalize()
        .map_err(|err| RuntimeError::InvalidConfig(format!("hmi dir unavailable: {err}").into()))?;
    let requested = requested
        .canonicalize()
        .map_err(|err| RuntimeError::InvalidConfig(format!("hmi asset not found: {err}").into()))?;
    if !requested.starts_with(&hmi_dir) {
        return Err(RuntimeError::InvalidConfig("invalid hmi asset path".into()));
    }
    if requested.extension().and_then(|value| value.to_str()) != Some("svg") {
        return Err(RuntimeError::InvalidConfig(
            "unsupported hmi asset type (only .svg is allowed)".into(),
        ));
    }
    std::fs::read_to_string(&requested).map_err(|err| {
        RuntimeError::InvalidConfig(format!("failed to read hmi asset '{}': {err}", name).into())
    })
}

fn apply_setup(
    bundle_root: &Option<PathBuf>,
    payload: SetupApplyRequest,
) -> Result<String, RuntimeError> {
    let defaults = setup_defaults(bundle_root);
    let bundle_path = payload
        .project_path
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(defaults.project_path.clone());
    let bundle_root = PathBuf::from(bundle_path);
    std::fs::create_dir_all(&bundle_root).map_err(|err| {
        RuntimeError::InvalidConfig(format!("failed to create project folder: {err}").into())
    })?;

    let resource_name = payload
        .resource_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(defaults.resource_name);
    let cycle_ms = payload.cycle_ms.unwrap_or(defaults.cycle_ms);
    let mut driver = payload
        .driver
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(defaults.driver);
    if driver == "auto" {
        driver = detect_default_driver();
    }

    let use_system_io = payload.use_system_io.unwrap_or(defaults.use_system_io);
    let write_system_io = payload.write_system_io.unwrap_or(defaults.write_system_io);
    let overwrite_system_io = payload.overwrite_system_io.unwrap_or(false);

    let runtime_path = bundle_root.join("runtime.toml");
    let runtime_text =
        crate::bundle_template::render_runtime_toml(&SmolStr::new(resource_name), cycle_ms);
    crate::config::validate_runtime_toml_text(&runtime_text)?;
    std::fs::write(&runtime_path, runtime_text).map_err(|err| {
        RuntimeError::InvalidConfig(format!("failed to write runtime.toml: {err}").into())
    })?;

    let io_path = bundle_root.join("io.toml");
    if use_system_io {
        if io_path.exists() {
            std::fs::remove_file(&io_path).map_err(|err| {
                RuntimeError::InvalidConfig(format!("failed to remove io.toml: {err}").into())
            })?;
        }
    } else {
        let template =
            crate::bundle_template::build_io_config_auto(driver.as_str()).map_err(|err| {
                RuntimeError::InvalidConfig(format!("io template error: {err}").into())
            })?;
        let io_text = crate::bundle_template::render_io_toml(&template);
        crate::config::validate_io_toml_text(&io_text)?;
        std::fs::write(&io_path, io_text).map_err(|err| {
            RuntimeError::InvalidConfig(format!("failed to write io.toml: {err}").into())
        })?;
    }

    if write_system_io {
        let options = SetupOptions {
            driver: Some(SmolStr::new(driver)),
            backend: None,
            force: overwrite_system_io,
            path: None,
        };
        crate::setup::run_setup(options)?;
    }

    Ok("✓ Setup applied. Restart the runtime to load the new configuration.".to_string())
}

pub struct WebServer {
    // Retained to keep the web thread alive for the lifetime of the server handle.
    #[allow(dead_code)]
    handle: thread::JoinHandle<()>,
    pub listen: String,
}

pub fn start_web_server(
    config: &WebConfig,
    control_state: Arc<ControlState>,
    discovery: Option<Arc<DiscoveryState>>,
    pairing: Option<Arc<PairingStore>>,
    bundle_root: Option<PathBuf>,
    tls_materials: Option<Arc<TlsMaterials>>,
) -> Result<WebServer, RuntimeError> {
    if !config.enabled {
        return Err(RuntimeError::ControlError("web disabled".into()));
    }
    let listen = config.listen.to_string();
    let server = if config.tls {
        let materials = tls_materials.as_ref().ok_or_else(|| {
            RuntimeError::ControlError(
                "web tls enabled but runtime.tls certificate settings are unavailable".into(),
            )
        })?;
        Server::https(&listen, materials.tiny_http_ssl_config())
            .map_err(|err| RuntimeError::ControlError(format!("web tls bind: {err}").into()))?
    } else {
        Server::http(&listen)
            .map_err(|err| RuntimeError::ControlError(format!("web bind: {err}").into()))?
    };
    let auth = config.auth;
    let web_url = format_web_url(&listen, config.tls);
    let auth_token = control_state.auth_token.clone();
    let discovery = discovery.unwrap_or_else(|| Arc::new(DiscoveryState::new()));
    let pairing = pairing.or_else(|| {
        bundle_root
            .as_ref()
            .map(|root| Arc::new(PairingStore::load(root.join("pairings.json"))))
    });
    let ide_state = Arc::new(WebIdeState::new(bundle_root.clone()));
    let ide_task_store: Arc<Mutex<HashMap<u64, IdeTaskJob>>> = Arc::new(Mutex::new(HashMap::new()));
    let ide_task_seq = Arc::new(AtomicU64::new(1));
    let bundle_root = bundle_root.clone();
    let handle = thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let method = request.method().clone();
            let url = request.url().to_string();
            let url_path = url.split('?').next().unwrap_or(url.as_str());
            if method == Method::Get && (url == "/" || url == "/setup") {
                let response = Response::from_string(INDEX_HTML)
                    .with_header(Header::from_bytes("Content-Type", "text/html").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && (url == "/hmi" || url == "/hmi/") {
                let response = Response::from_string(HMI_HTML)
                    .with_header(Header::from_bytes("Content-Type", "text/html").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url_path.starts_with("/hmi/assets/") {
                let Some(project_root) = bundle_root
                    .clone()
                    .or_else(|| control_state.project_root.clone())
                else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "project root unavailable" }).to_string(),
                    )
                    .with_status_code(StatusCode(400))
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                    let _ = request.respond(response);
                    continue;
                };
                let encoded = url_path.trim_start_matches("/hmi/assets/");
                if encoded.is_empty() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing asset path" }).to_string(),
                    )
                    .with_status_code(StatusCode(400))
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                    let _ = request.respond(response);
                    continue;
                }
                let asset = decode_url_component(encoded);
                match read_hmi_asset_file(&project_root, asset.as_str()) {
                    Ok(svg) => {
                        let response = Response::from_string(svg).with_header(
                            Header::from_bytes("Content-Type", "image/svg+xml").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(err) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": err.to_string() }).to_string(),
                        )
                        .with_status_code(StatusCode(404))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                }
                continue;
            }
            if method == Method::Get && url_path == HMI_WS_ROUTE {
                let request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let accept_key = match websocket_accept_key(&request) {
                    Ok(key) => key,
                    Err(error) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": error }).to_string(),
                        )
                        .with_status_code(StatusCode(400))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let response = Response::empty(StatusCode(101)).with_header(
                    Header::from_bytes("Sec-WebSocket-Accept", accept_key.as_bytes()).unwrap(),
                );
                let stream = request.upgrade("websocket", response);
                spawn_hmi_websocket_session(stream, control_state.clone(), request_token);
                continue;
            }
            if method == Method::Get && (url == "/ide" || url == "/ide/") {
                let response = Response::from_string(IDE_HTML)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(Header::from_bytes("Content-Type", "text/html").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/ide.css" {
                let response = Response::from_string(IDE_CSS)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(Header::from_bytes("Content-Type", "text/css").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/ide.js" {
                let response = Response::from_string(IDE_JS)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(
                        Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                    );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/assets/ide-monaco.20260215.js" {
                let response = Response::from_string(IDE_MONACO_BUNDLE_JS)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(
                        Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                    );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/assets/ide-monaco.20260215.css" {
                let response = Response::from_string(IDE_MONACO_BUNDLE_CSS)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(Header::from_bytes("Content-Type", "text/css").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/assets/logo.svg" {
                let response = Response::from_string(IDE_LOGO_SVG)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(Header::from_bytes("Content-Type", "image/svg+xml").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/wasm/worker.js" {
                let response = Response::from_string(IDE_WASM_WORKER_JS)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(
                        Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                    );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/wasm/analysis-client.js" {
                let response = Response::from_string(IDE_WASM_CLIENT_JS)
                    .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    .with_header(
                        Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                    );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/ide/wasm/trust_wasm_analysis.js" {
                let wasm_pkg_dir = std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                    .map(|p| p.join("../../target/browser-analysis-wasm/pkg"))
                    .or_else(|| {
                        std::env::var("CARGO_MANIFEST_DIR").ok().map(|d| {
                            PathBuf::from(d).join("../../target/browser-analysis-wasm/pkg")
                        })
                    })
                    .unwrap_or_else(|| PathBuf::from("target/browser-analysis-wasm/pkg"));
                let js_path = wasm_pkg_dir.join("trust_wasm_analysis.js");
                match std::fs::read_to_string(&js_path) {
                    Ok(js_content) => {
                        let response = Response::from_string(js_content)
                            .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                            .with_header(
                                Header::from_bytes("Content-Type", "application/javascript")
                                    .unwrap(),
                            );
                        let _ = request.respond(response);
                    }
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "WASM JS glue not found. Run scripts/build_browser_analysis_wasm_spike.sh" }).to_string(),
                        )
                        .with_status_code(StatusCode(404))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/ide/wasm/trust_wasm_analysis_bg.wasm" {
                let wasm_pkg_dir = std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                    .map(|p| p.join("../../target/browser-analysis-wasm/pkg"))
                    .or_else(|| {
                        std::env::var("CARGO_MANIFEST_DIR").ok().map(|d| {
                            PathBuf::from(d).join("../../target/browser-analysis-wasm/pkg")
                        })
                    })
                    .unwrap_or_else(|| PathBuf::from("target/browser-analysis-wasm/pkg"));
                let wasm_path = wasm_pkg_dir.join("trust_wasm_analysis_bg.wasm");
                match std::fs::read(&wasm_path) {
                    Ok(wasm_bytes) => {
                        let cursor = std::io::Cursor::new(wasm_bytes);
                        let response = Response::new(
                            StatusCode(200),
                            vec![
                                Header::from_bytes("Cache-Control", "no-store").unwrap(),
                                Header::from_bytes("Content-Type", "application/wasm").unwrap(),
                            ],
                            cursor,
                            None,
                            None,
                        );
                        let _ = request.respond(response);
                    }
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "WASM binary not found. Run scripts/build_browser_analysis_wasm_spike.sh" }).to_string(),
                        )
                        .with_status_code(StatusCode(404))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/hmi/export.json" {
                let schema_response = handle_request_value(
                    json!({
                        "id": 1_u64,
                        "type": "hmi.schema.get"
                    }),
                    &control_state,
                    None,
                );
                let schema_payload = serde_json::to_value(schema_response)
                    .unwrap_or_else(|_| json!({ "ok": false }));
                let ok = schema_payload
                    .get("ok")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                if !ok {
                    let response =
                        Response::from_string(json!({ "error": "schema unavailable" }).to_string())
                            .with_status_code(503)
                            .with_header(
                                Header::from_bytes("Content-Type", "application/json").unwrap(),
                            );
                    let _ = request.respond(response);
                    continue;
                }
                let schema = schema_payload
                    .get("result")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let exported_at_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let descriptor = control_state
                    .hmi_descriptor
                    .lock()
                    .ok()
                    .and_then(|state| state.customization.dir_descriptor().cloned());
                let payload = json!({
                    "version": 2_u32,
                    "exported_at_ms": exported_at_ms,
                    "entrypoint": "hmi/index.html",
                    "routes": ["/hmi", "/hmi/app.js", "/hmi/styles.css", "/api/control", HMI_WS_ROUTE],
                    "config": {
                        "poll_ms": 500_u32,
                        "ws_route": HMI_WS_ROUTE,
                        "schema": schema,
                        "descriptor": descriptor
                    },
                    "assets": {
                        "hmi/index.html": HMI_HTML,
                        "hmi/styles.css": HMI_CSS,
                        "hmi/app.js": HMI_JS
                    }
                });
                let response = Response::from_string(payload.to_string())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
                    .with_header(
                        Header::from_bytes(
                            "Content-Disposition",
                            "attachment; filename=\"trust-hmi-export.json\"",
                        )
                        .unwrap(),
                    );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/styles.css" {
                let response = Response::from_string(APP_CSS)
                    .with_header(Header::from_bytes("Content-Type", "text/css").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/hmi/styles.css" {
                let response = Response::from_string(HMI_CSS)
                    .with_header(Header::from_bytes("Content-Type", "text/css").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/app.js" {
                let response = Response::from_string(APP_JS).with_header(
                    Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/hmi/app.js" {
                let response = Response::from_string(HMI_JS).with_header(
                    Header::from_bytes("Content-Type", "application/javascript").unwrap(),
                );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get
                && control_state
                    .historian
                    .as_ref()
                    .and_then(|hist| hist.prometheus_path())
                    == Some(url.as_str())
            {
                let _request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let metrics = control_state
                    .metrics
                    .lock()
                    .ok()
                    .map(|guard| guard.snapshot())
                    .unwrap_or_default();
                let body = control_state
                    .historian
                    .as_ref()
                    .map(|service| service.render_prometheus(&metrics))
                    .unwrap_or_else(|| crate::historian::render_prometheus(&metrics, None));
                let response = Response::from_string(body).with_header(
                    Header::from_bytes("Content-Type", "text/plain; version=0.0.4").unwrap(),
                );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url.starts_with("/api/qr") {
                let text = url.split('?').nth(1).and_then(|query| {
                    query.split('&').find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        if parts.next()? == "text" {
                            Some(parts.next().unwrap_or("").to_string())
                        } else {
                            None
                        }
                    })
                });
                if let Some(encoded) = text {
                    let decoded =
                        urlencoding::decode(&encoded).unwrap_or_else(|_| encoded.as_str().into());
                    match render_qr_svg(decoded.as_ref()) {
                        Ok(svg) => {
                            let response = Response::from_string(svg).with_header(
                                Header::from_bytes("Content-Type", "image/svg+xml").unwrap(),
                            );
                            let _ = request.respond(response);
                        }
                        Err(err) => {
                            let response = Response::from_string(format!("error: {err}"))
                                .with_status_code(500);
                            let _ = request.respond(response);
                        }
                    }
                } else {
                    let response = Response::from_string("missing text").with_status_code(400);
                    let _ = request.respond(response);
                }
                continue;
            }
            if method == Method::Get && url == "/api/discovery" {
                let items = discovery
                    .snapshot()
                    .into_iter()
                    .map(|entry| {
                        json!({
                            "id": entry.id.as_str(),
                            "name": entry.name.as_str(),
                            "addresses": entry.addresses,
                            "web_port": entry.web_port,
                            "mesh_port": entry.mesh_port,
                            "control": entry.control.as_ref().map(|v| v.as_str()),
                        })
                    })
                    .collect::<Vec<_>>();
                let body = json!({ "items": items }).to_string();
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url.starts_with("/api/probe") {
                let target = url
                    .split_once('?')
                    .map(|(_, query)| query)
                    .and_then(|query| {
                        query
                            .split('&')
                            .find(|part| part.starts_with("url="))
                            .map(|part| part.trim_start_matches("url="))
                    })
                    .map(decode_url_component);
                let target = match target {
                    Some(value)
                        if value.starts_with("http://") || value.starts_with("https://") =>
                    {
                        value.trim_end_matches('/').to_string()
                    }
                    _ => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid url" }).to_string(),
                        )
                        .with_status_code(400);
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let control_url = format!("{target}/api/control");
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_millis(500))
                    .timeout_read(Duration::from_millis(800))
                    .build();
                let body = json!({ "id": 1u64, "type": "status" }).to_string();
                let response_body = agent
                    .post(&control_url)
                    .set("Content-Type", "application/json")
                    .send_string(&body);
                let payload = match response_body {
                    Ok(resp) => {
                        let text = resp.into_string().unwrap_or_default();
                        parse_probe_response(&text)
                    }
                    Err(ureq::Error::Status(401, _)) => {
                        json!({ "ok": false, "error": "auth_required" })
                    }
                    Err(_) => json!({ "ok": false, "error": "unreachable" }),
                };
                let response = Response::from_string(payload.to_string())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/api/setup/defaults" {
                let defaults = setup_defaults(&bundle_root);
                let body = serde_json::to_string(&defaults).unwrap_or_else(|_| "{}".to_string());
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/setup/apply" {
                let _request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Engineer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string("invalid body").with_status_code(400);
                    let _ = request.respond(response);
                    continue;
                }
                let payload: SetupApplyRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string("invalid json").with_status_code(400);
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let response_body = match apply_setup(&bundle_root, payload) {
                    Ok(message) => message,
                    Err(err) => format!("error: {err}"),
                };
                let response = Response::from_string(response_body)
                    .with_header(Header::from_bytes("Content-Type", "text/plain").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/api/io/config" {
                let body = match load_io_config(&bundle_root) {
                    Ok(config) => {
                        serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string())
                    }
                    Err(err) => json!({ "error": err.to_string() }).to_string(),
                };
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/io/config" {
                let _request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Engineer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string("invalid body").with_status_code(400);
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IoConfigRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string("invalid json").with_status_code(400);
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let project_root = default_bundle_root(&bundle_root);
                let io_path = project_root.join("io.toml");
                let use_system = payload.use_system_io.unwrap_or(false);
                let response_body = if use_system {
                    if io_path.exists() {
                        if let Err(err) = std::fs::remove_file(&io_path) {
                            format!("error: failed to remove io.toml: {err}")
                        } else {
                            "✓ Using system I/O config. Restart the runtime to apply.".to_string()
                        }
                    } else {
                        "✓ Using system I/O config. Restart the runtime to apply.".to_string()
                    }
                } else {
                    match driver_configs_from_payload(&payload) {
                        Ok(drivers) => {
                            let safe_state = payload.safe_state.clone().unwrap_or_default();
                            let io_text = render_io_toml(drivers, safe_state);
                            match crate::config::validate_io_toml_text(&io_text) {
                                Ok(()) => match std::fs::write(&io_path, io_text) {
                                    Ok(_) => "✓ I/O config saved. Restart the runtime to apply."
                                        .to_string(),
                                    Err(err) => format!("error: failed to write io.toml: {err}"),
                                },
                                Err(err) => format!("error: {err}"),
                            }
                        }
                        Err(err) => format!("error: {err}"),
                    }
                };
                let response = Response::from_string(response_body)
                    .with_header(Header::from_bytes("Content-Type", "text/plain").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/io/modbus-test" {
                let _request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string("invalid body").with_status_code(400);
                    let _ = request.respond(response);
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string("invalid json").with_status_code(400);
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let address = payload
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let port = payload.get("port").and_then(|v| v.as_u64()).unwrap_or(502);
                let timeout_ms = payload
                    .get("timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(500);
                let target = if address.contains(':') {
                    address.to_string()
                } else {
                    format!("{address}:{port}")
                };
                let result = target
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut addrs| addrs.next())
                    .ok_or_else(|| RuntimeError::InvalidConfig("invalid address".into()))
                    .and_then(|addr| {
                        std::net::TcpStream::connect_timeout(
                            &addr,
                            std::time::Duration::from_millis(timeout_ms),
                        )
                        .map_err(|err| {
                            RuntimeError::ControlError(format!("connect failed: {err}").into())
                        })
                    });
                let body = match result {
                    Ok(_) => json!({ "ok": true }).to_string(),
                    Err(err) => json!({ "ok": false, "error": err.to_string() }).to_string(),
                };
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/api/program" {
                let project_root = default_bundle_root(&bundle_root);
                let program_path = project_root.join("program.stbc");
                let updated_ms = program_path
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_millis());
                let sources = list_sources(&project_root);
                let body = json!({
                    "program": "program.stbc",
                    "updated_ms": updated_ms,
                    "sources": sources,
                })
                .to_string();
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url.starts_with("/api/source") {
                let file = url.split('?').nth(1).and_then(|query| {
                    query.split('&').find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        if parts.next()? == "file" {
                            Some(parts.next().unwrap_or("").to_string())
                        } else {
                            None
                        }
                    })
                });
                let Some(encoded) = file else {
                    let response = Response::from_string("missing file").with_status_code(400);
                    let _ = request.respond(response);
                    continue;
                };
                let decoded =
                    urlencoding::decode(&encoded).unwrap_or_else(|_| encoded.as_str().into());
                let project_root = default_bundle_root(&bundle_root);
                match read_source_file(&project_root, decoded.as_ref()) {
                    Ok(text) => {
                        let response = Response::from_string(text).with_header(
                            Header::from_bytes("Content-Type", "text/plain; charset=utf-8")
                                .unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(err) => {
                        let response =
                            Response::from_string(format!("error: {err}")).with_status_code(404);
                        let _ = request.respond(response);
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/api/ide/capabilities" {
                let (web_role, _request_token) = match check_auth_with_role(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let caps = ide_state.capabilities(web_role.allows(AccessRole::Engineer));
                let response =
                    Response::from_string(json!({ "ok": true, "result": caps }).to_string())
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/ide/session" {
                let (web_role, _request_token) = match check_auth_with_role(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload = if body.trim().is_empty() {
                    IdeSessionRequest { role: None }
                } else {
                    match serde_json::from_str::<IdeSessionRequest>(&body) {
                        Ok(value) => value,
                        Err(_) => {
                            let response = Response::from_string(
                                json!({ "ok": false, "error": "invalid json" }).to_string(),
                            )
                            .with_status_code(StatusCode(400));
                            let _ = request.respond(response);
                            continue;
                        }
                    }
                };
                let role = payload
                    .role
                    .as_deref()
                    .and_then(IdeRole::parse)
                    .unwrap_or(IdeRole::Viewer);
                if matches!(role, IdeRole::Editor) && !web_role.allows(AccessRole::Engineer) {
                    let response = Response::from_string(
                        json!({
                            "ok": false,
                            "error": "editor session requires engineer/admin web role"
                        })
                        .to_string(),
                    )
                    .with_status_code(StatusCode(403))
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                    let _ = request.respond(response);
                    continue;
                }
                match ide_state.create_session(role) {
                    Ok(session) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": session }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/api/ide/project" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                match ide_state.project_selection(session_token.as_str()) {
                    Ok(selection) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": selection }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/project/open" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeProjectOpenRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                match ide_state.set_active_project(session_token.as_str(), payload.path.as_str()) {
                    Ok(selection) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": selection }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/api/ide/files" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                match ide_state.list_sources(session_token.as_str()) {
                    Ok(files) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": { "files": files } }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/api/ide/tree" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                match ide_state.list_tree(session_token.as_str()) {
                    Ok(tree) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": { "tree": tree } }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url.starts_with("/api/ide/browse") {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let path = query_value(url.as_str(), "path");
                match ide_state.browse_directory(session_token.as_str(), path.as_deref()) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url.starts_with("/api/ide/file") {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let path = url.split('?').nth(1).and_then(|query| {
                    query.split('&').find_map(|pair| {
                        let mut parts = pair.splitn(2, '=');
                        if parts.next()? == "path" {
                            Some(decode_url_component(parts.next().unwrap_or_default()))
                        } else {
                            None
                        }
                    })
                });
                let Some(path) = path else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing path" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                };
                match ide_state.open_source(session_token.as_str(), path.as_str()) {
                    Ok(mut snapshot) => {
                        if !ide_write_enabled(&control_state) {
                            snapshot.read_only = true;
                        }
                        let response = Response::from_string(
                            json!({ "ok": true, "result": snapshot }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/file" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeWriteRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                match ide_state.apply_source(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.expected_version,
                    payload.content,
                    ide_write_enabled(&control_state),
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/fs/create" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeFsCreateRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let is_directory = payload.kind.as_deref().is_some_and(|kind| {
                    kind.eq_ignore_ascii_case("directory") || kind.eq_ignore_ascii_case("dir")
                });
                match ide_state.create_entry(
                    session_token.as_str(),
                    payload.path.as_str(),
                    is_directory,
                    payload.content,
                    ide_write_enabled(&control_state),
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && (url == "/api/ide/fs/rename" || url == "/api/ide/fs/move")
            {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeFsRenameRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                match ide_state.rename_entry(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.new_path.as_str(),
                    ide_write_enabled(&control_state),
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/fs/delete" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeFsDeleteRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                match ide_state.delete_entry(
                    session_token.as_str(),
                    payload.path.as_str(),
                    ide_write_enabled(&control_state),
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url.starts_with("/api/ide/fs/audit") {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let limit = parse_limit(url.as_str()).unwrap_or(40).clamp(1, 200) as usize;
                match ide_state.fs_audit(session_token.as_str(), limit) {
                    Ok(events) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": events }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/api/ide/health" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                match ide_state.health(session_token.as_str()) {
                    Ok(health) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": health }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/frontend-telemetry" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeFrontendTelemetryRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let telemetry = WebIdeFrontendTelemetry {
                    bootstrap_failures: payload.bootstrap_failures.unwrap_or(0),
                    analysis_timeouts: payload.analysis_timeouts.unwrap_or(0),
                    worker_restarts: payload.worker_restarts.unwrap_or(0),
                    autosave_failures: payload.autosave_failures.unwrap_or(0),
                };
                match ide_state.record_frontend_telemetry(session_token.as_str(), telemetry) {
                    Ok(aggregated) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": aggregated }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/api/ide/presence-model" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                if let Err(error) = ide_state.health(session_token.as_str()) {
                    let _ = request.respond(ide_error_response(error));
                    continue;
                }
                let response = Response::from_string(
                    json!({
                        "ok": true,
                        "result": {
                            "mode": "out_of_scope_phase_1",
                            "summary": "Live collaborative cursor/presence is intentionally deferred for first production release.",
                            "tracking": "See docs/guides/WEB_IDE_COLLABORATION_MODEL.md",
                        }
                    })
                    .to_string(),
                )
                .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/ide/diagnostics" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeDiagnosticsRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                match ide_state.diagnostics(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.content,
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/hover" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeHoverRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let position = trust_wasm_analysis::Position {
                    line: payload.position.line,
                    character: payload.position.character,
                };
                match ide_state.hover(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.content,
                    position,
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/completion" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeCompletionRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let position = trust_wasm_analysis::Position {
                    line: payload.position.line,
                    character: payload.position.character,
                };
                match ide_state.completion(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.content,
                    position,
                    payload.limit,
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/definition" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeHoverRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let position = trust_wasm_analysis::Position {
                    line: payload.position.line,
                    character: payload.position.character,
                };
                match ide_state.definition(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.content,
                    position,
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/references" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeReferencesRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let position = trust_wasm_analysis::Position {
                    line: payload.position.line,
                    character: payload.position.character,
                };
                match ide_state.references(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.content,
                    position,
                    payload.include_declaration.unwrap_or(true),
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post && url == "/api/ide/rename" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeRenameRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let position = trust_wasm_analysis::Position {
                    line: payload.position.line,
                    character: payload.position.character,
                };
                match ide_state.rename_symbol(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.content,
                    position,
                    payload.new_name.as_str(),
                    ide_write_enabled(&control_state),
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url.starts_with("/api/ide/search") {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let query = query_value(url.as_str(), "q").unwrap_or_default();
                let include = query_value(url.as_str(), "include");
                let exclude = query_value(url.as_str(), "exclude");
                let limit = parse_limit(url.as_str()).unwrap_or(50).clamp(1, 500) as usize;
                match ide_state.workspace_search(
                    session_token.as_str(),
                    query.as_str(),
                    include.as_deref(),
                    exclude.as_deref(),
                    limit,
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url.starts_with("/api/ide/symbols") {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let query = query_value(url.as_str(), "q").unwrap_or_default();
                let limit = parse_limit(url.as_str()).unwrap_or(100).clamp(1, 1000) as usize;
                let path = query_value(url.as_str(), "path");
                let result = if let Some(path) = path {
                    ide_state.file_symbols(
                        session_token.as_str(),
                        path.as_str(),
                        query.as_str(),
                        limit,
                    )
                } else {
                    ide_state.workspace_symbols(session_token.as_str(), query.as_str(), limit)
                };
                match result {
                    Ok(items) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": items }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Post
                && (url == "/api/ide/build" || url == "/api/ide/test" || url == "/api/ide/validate")
            {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                if let Err(error) = ide_state.require_editor_session(session_token.as_str()) {
                    let _ = request.respond(ide_error_response(error));
                    continue;
                }
                let kind = if url.ends_with("/build") {
                    "build"
                } else if url.ends_with("/test") {
                    "test"
                } else {
                    "validate"
                };
                let Some(project_root) = ide_state.active_project_root() else {
                    let response = Response::from_string(
                        json!({
                            "ok": false,
                            "error": "no active project selected; open a folder in the IDE first"
                        })
                        .to_string(),
                    )
                    .with_status_code(StatusCode(400))
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                    let _ = request.respond(response);
                    continue;
                };
                let snapshot = start_ide_task_job(
                    kind,
                    project_root,
                    ide_task_store.clone(),
                    ide_task_seq.clone(),
                );
                let response =
                    Response::from_string(json!({ "ok": true, "result": snapshot }).to_string())
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/ide/format" {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: IdeFormatRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                match ide_state.format_source(
                    session_token.as_str(),
                    payload.path.as_str(),
                    payload.content,
                ) {
                    Ok(result) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": result }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    Err(error) => {
                        let _ = request.respond(ide_error_response(error));
                    }
                }
                continue;
            }
            if method == Method::Get && url.starts_with("/api/ide/task") {
                let Some(session_token) = ide_session_token(&request) else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing X-Trust-Ide-Session" }).to_string(),
                    )
                    .with_status_code(StatusCode(401));
                    let _ = request.respond(response);
                    continue;
                };
                if let Err(error) = ide_state.health(session_token.as_str()) {
                    let _ = request.respond(ide_error_response(error));
                    continue;
                }
                let Some(id_text) = query_value(url.as_str(), "id") else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "missing id" }).to_string(),
                    )
                    .with_status_code(StatusCode(400))
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                    let _ = request.respond(response);
                    continue;
                };
                let Ok(job_id) = id_text.parse::<u64>() else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid id" }).to_string(),
                    )
                    .with_status_code(StatusCode(400))
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                    let _ = request.respond(response);
                    continue;
                };
                let snapshot = ide_task_snapshot(ide_task_store.clone(), job_id);
                match snapshot {
                    Some(task) => {
                        let response = Response::from_string(
                            json!({ "ok": true, "result": task }).to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                    None => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "task not found" }).to_string(),
                        )
                        .with_status_code(StatusCode(404))
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        );
                        let _ = request.respond(response);
                    }
                }
                continue;
            }
            if method == Method::Get && url == "/api/pairings" {
                let _request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Admin,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let list = pairing
                    .as_ref()
                    .map(|store| store.list())
                    .unwrap_or_default();
                let body = json!({ "items": list }).to_string();
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/pair/start" {
                let _request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Admin,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let body = if let Some(store) = pairing.as_ref() {
                    let code = store.start_pairing();
                    json!({
                        "code": code.code,
                        "expires_at": code.expires_at,
                    })
                } else {
                    json!({ "error": "pairing unavailable" })
                };
                let response = Response::from_string(body.to_string())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/pair/claim" {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let code = payload.get("code").and_then(|value| value.as_str());
                let requested_role = payload
                    .get("role")
                    .and_then(|value| value.as_str())
                    .and_then(AccessRole::parse);
                let token = code.and_then(|value| {
                    pairing
                        .as_ref()
                        .and_then(|store| store.claim(value, requested_role))
                });
                let body = if let Some(token) = token {
                    json!({ "token": token })
                } else {
                    json!({ "error": "invalid code" })
                };
                let response = Response::from_string(body.to_string())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url == "/api/invite" {
                let _request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Admin,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let token = auth_token
                    .lock()
                    .ok()
                    .and_then(|guard| guard.as_ref().map(|value| value.to_string()));
                let body = json!({
                    "endpoint": web_url,
                    "token": token,
                })
                .to_string();
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url.starts_with("/api/events") {
                let request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let limit = parse_limit(&url).unwrap_or(50);
                let response = dispatch_control_request(
                    json!({ "id": 1, "type": "events.tail", "params": { "limit": limit } }),
                    &control_state,
                    Some("web"),
                    request_token.as_deref(),
                );
                let body = serde_json::to_string(&response).unwrap_or_else(|_| "{}".into());
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Get && url.starts_with("/api/faults") {
                let request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let limit = parse_limit(&url).unwrap_or(50);
                let response = dispatch_control_request(
                    json!({ "id": 1, "type": "faults", "params": { "limit": limit } }),
                    &control_state,
                    Some("web"),
                    request_token.as_deref(),
                );
                let body = serde_json::to_string(&response).unwrap_or_else(|_| "{}".into());
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/deploy" {
                let request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Admin,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: DeployRequest = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let Some(bundle_root) = bundle_root.as_ref() else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "project folder unavailable" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                };
                let result = apply_deploy(bundle_root, payload);
                let body = match result {
                    Ok(result) => {
                        if let Some(restart) = result.restart.as_ref() {
                            let _ = dispatch_control_request(
                                json!({ "id": 1, "type": "restart", "params": { "mode": restart } }),
                                &control_state,
                                Some("web"),
                                request_token.as_deref(),
                            );
                        }
                        json!({ "ok": true, "written": result.written, "restart": result.restart })
                    }
                    Err(err) => json!({ "ok": false, "error": err.to_string() }),
                };
                let response = Response::from_string(body.to_string())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/rollback" {
                let request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Admin,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: RollbackRequest =
                    serde_json::from_str(&body).unwrap_or(RollbackRequest { restart: None });
                let Some(bundle_root) = bundle_root.as_ref() else {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "project folder unavailable" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                };
                let root = bundle_root
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| bundle_root.clone());
                let result = apply_rollback(&root);
                let body = match result {
                    Ok(result) => {
                        if let Some(restart) = payload.restart.as_ref() {
                            let _ = dispatch_control_request(
                                json!({ "id": 1, "type": "restart", "params": { "mode": restart } }),
                                &control_state,
                                Some("web"),
                                request_token.as_deref(),
                            );
                        }
                        json!({
                            "ok": true,
                            "current": result.current.display().to_string(),
                            "previous": result.previous.display().to_string(),
                        })
                    }
                    Err(err) => json!({ "ok": false, "error": err.to_string() }),
                };
                let response = Response::from_string(body.to_string())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            if method == Method::Post && url == "/api/control" {
                let request_token = match check_auth(
                    &request,
                    auth,
                    &auth_token,
                    pairing.as_deref(),
                    AccessRole::Viewer,
                ) {
                    Ok(token) => token,
                    Err(error) => {
                        let _ = request.respond(auth_error_response(error));
                        continue;
                    }
                };
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let response = Response::from_string(
                        json!({ "ok": false, "error": "invalid body" }).to_string(),
                    )
                    .with_status_code(StatusCode(400));
                    let _ = request.respond(response);
                    continue;
                }
                let payload: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(value) => value,
                    Err(_) => {
                        let response = Response::from_string(
                            json!({ "ok": false, "error": "invalid json" }).to_string(),
                        )
                        .with_status_code(StatusCode(400));
                        let _ = request.respond(response);
                        continue;
                    }
                };
                let response = dispatch_control_request(
                    payload,
                    &control_state,
                    Some("web"),
                    request_token.as_deref(),
                );
                let body = serde_json::to_string(&response).unwrap_or_else(|_| "{}".into());
                let response = Response::from_string(body)
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                continue;
            }
            let response = Response::from_string("not found").with_status_code(StatusCode(404));
            let _ = request.respond(response);
        }
    });

    Ok(WebServer { handle, listen })
}

fn websocket_accept_key(request: &tiny_http::Request) -> Result<String, &'static str> {
    let upgrade = header_value(request, "Upgrade").ok_or("missing Upgrade header")?;
    if !upgrade.eq_ignore_ascii_case("websocket") {
        return Err("invalid websocket upgrade");
    }
    let connection = header_value(request, "Connection").ok_or("missing Connection header")?;
    if !connection.to_ascii_lowercase().contains("upgrade") {
        return Err("invalid Connection upgrade");
    }
    let key = header_value(request, "Sec-WebSocket-Key").ok_or("missing Sec-WebSocket-Key")?;
    Ok(tungstenite::handshake::derive_accept_key(key.as_bytes()))
}

fn header_value(request: &tiny_http::Request, key: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(key))
        .map(|header| header.value.as_str().trim().to_string())
}

fn spawn_hmi_websocket_session(
    stream: Box<dyn tiny_http::ReadWrite + Send>,
    control_state: Arc<ControlState>,
    request_token: Option<String>,
) {
    thread::spawn(move || {
        if let Err(err) = run_hmi_websocket_session(stream, control_state, request_token) {
            tracing::debug!("hmi websocket session closed: {err}");
        }
    });
}

fn run_hmi_websocket_session(
    stream: Box<dyn tiny_http::ReadWrite + Send>,
    control_state: Arc<ControlState>,
    request_token: Option<String>,
) -> Result<(), String> {
    use tungstenite::protocol::Role;

    let mut socket = tungstenite::protocol::WebSocket::from_raw_socket(stream, Role::Server, None);
    let mut request_id = 10_000_u64;
    let mut last_schema_revision = 0_u64;
    let mut widget_ids = Vec::new();
    let mut last_values = serde_json::Map::new();
    let mut last_alarm_payload: Option<serde_json::Value> = None;
    let mut next_schema_poll = Instant::now();
    let mut next_alarm_poll = Instant::now();

    if let Some(schema_result) = hmi_control_result(
        control_state.as_ref(),
        &mut request_id,
        "hmi.schema.get",
        None,
        request_token.as_deref(),
    ) {
        last_schema_revision = schema_result
            .get("schema_revision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        widget_ids = hmi_widget_ids(&schema_result);
    }

    loop {
        let values_params = if widget_ids.is_empty() {
            None
        } else {
            Some(json!({ "ids": widget_ids }))
        };
        let values_result = hmi_control_result(
            control_state.as_ref(),
            &mut request_id,
            "hmi.values.get",
            values_params,
            request_token.as_deref(),
        )
        .ok_or_else(|| "hmi.values.get failed".to_string())?;

        if let Some(delta) = hmi_values_delta(&values_result, &mut last_values) {
            hmi_ws_send_json(
                &mut socket,
                &json!({
                    "type": "hmi.values.delta",
                    "result": delta,
                }),
            )?;
        }

        let now = Instant::now();
        if now >= next_schema_poll {
            next_schema_poll = now + HMI_WS_SCHEMA_POLL_INTERVAL;
            if let Some(schema_result) = hmi_control_result(
                control_state.as_ref(),
                &mut request_id,
                "hmi.schema.get",
                None,
                request_token.as_deref(),
            ) {
                let revision = schema_result
                    .get("schema_revision")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(last_schema_revision);
                if revision != last_schema_revision {
                    last_schema_revision = revision;
                    widget_ids = hmi_widget_ids(&schema_result);
                    hmi_ws_send_json(
                        &mut socket,
                        &json!({
                            "type": "hmi.schema.revision",
                            "result": { "schema_revision": revision }
                        }),
                    )?;
                }
            }
        }

        if now >= next_alarm_poll {
            next_alarm_poll = now + HMI_WS_ALARMS_POLL_INTERVAL;
            if let Some(alarms_result) = hmi_control_result(
                control_state.as_ref(),
                &mut request_id,
                "hmi.alarms.get",
                Some(json!({ "limit": 50_u64 })),
                request_token.as_deref(),
            ) {
                if last_alarm_payload.as_ref() != Some(&alarms_result) {
                    last_alarm_payload = Some(alarms_result.clone());
                    hmi_ws_send_json(
                        &mut socket,
                        &json!({
                            "type": "hmi.alarms.event",
                            "result": alarms_result
                        }),
                    )?;
                }
            }
        }

        std::thread::sleep(HMI_WS_VALUES_POLL_INTERVAL);
    }
}

fn hmi_control_result(
    control_state: &ControlState,
    request_id: &mut u64,
    request_type: &str,
    params: Option<serde_json::Value>,
    request_token: Option<&str>,
) -> Option<serde_json::Value> {
    *request_id = request_id.saturating_add(1);
    let mut payload = json!({
        "id": *request_id,
        "type": request_type,
    });
    if let Some(params) = params {
        payload["params"] = params;
    }
    let response = dispatch_control_request(payload, control_state, Some("web/ws"), request_token);
    let response = serde_json::to_value(response).ok()?;
    if !response
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    response.get("result").cloned()
}

fn hmi_widget_ids(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("widgets")
        .and_then(serde_json::Value::as_array)
        .map(|widgets| {
            widgets
                .iter()
                .filter_map(|widget| widget.get("id").and_then(serde_json::Value::as_str))
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn hmi_values_delta(
    values_result: &serde_json::Value,
    last_values: &mut serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    let values = values_result.get("values")?.as_object()?;
    let mut delta = serde_json::Map::new();
    for (id, entry) in values {
        if last_values.get(id) != Some(entry) {
            delta.insert(id.clone(), entry.clone());
        }
    }
    last_values.retain(|id, _| values.contains_key(id));
    for (id, entry) in values {
        last_values.insert(id.clone(), entry.clone());
    }
    if delta.is_empty() {
        return None;
    }
    Some(json!({
        "connected": values_result
            .get("connected")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        "timestamp_ms": values_result.get("timestamp_ms").cloned().unwrap_or(serde_json::Value::Null),
        "values": delta,
    }))
}

fn hmi_ws_send_json<S>(
    socket: &mut tungstenite::protocol::WebSocket<S>,
    payload: &serde_json::Value,
) -> Result<(), String>
where
    S: std::io::Read + std::io::Write,
{
    socket
        .send(tungstenite::Message::Text(payload.to_string()))
        .map_err(|err| err.to_string())
}

fn check_auth(
    request: &tiny_http::Request,
    auth_mode: WebAuthMode,
    token: &Arc<Mutex<Option<smol_str::SmolStr>>>,
    pairing: Option<&PairingStore>,
    required_role: AccessRole,
) -> Result<Option<String>, &'static str> {
    check_auth_with_role(request, auth_mode, token, pairing, required_role)
        .map(|(_role, request_token)| request_token)
}

fn check_auth_with_role(
    request: &tiny_http::Request,
    auth_mode: WebAuthMode,
    token: &Arc<Mutex<Option<smol_str::SmolStr>>>,
    pairing: Option<&PairingStore>,
    required_role: AccessRole,
) -> Result<(AccessRole, Option<String>), &'static str> {
    let Some((role, request_token)) = resolve_web_role(request, auth_mode, token, pairing) else {
        return Err("unauthorized");
    };
    if !role.allows(required_role) {
        return Err("forbidden");
    }
    Ok((role, request_token))
}

fn resolve_web_role(
    request: &tiny_http::Request,
    auth_mode: WebAuthMode,
    token: &Arc<Mutex<Option<smol_str::SmolStr>>>,
    pairing: Option<&PairingStore>,
) -> Option<(AccessRole, Option<String>)> {
    if matches!(auth_mode, WebAuthMode::Local) {
        return Some((AccessRole::Admin, None));
    }
    let expected = token.lock().ok().and_then(|guard| guard.as_ref().cloned());
    let header = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("X-Trust-Token"))
        .map(|header| header.value.as_str().to_string());
    if let Some(expected) = expected {
        if header.as_deref() == Some(expected.as_str()) {
            return Some((AccessRole::Admin, header));
        }
    }
    let header = header?;
    pairing
        .as_ref()
        .and_then(|store| store.validate_with_role(header.as_str()))
        .map(|role| (role, Some(header)))
}

fn auth_error_response(error: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let status = if error == "forbidden" {
        StatusCode(403)
    } else {
        StatusCode(401)
    };
    Response::from_string(json!({ "ok": false, "error": error }).to_string())
        .with_status_code(status)
}

fn dispatch_control_request(
    mut payload: serde_json::Value,
    control_state: &ControlState,
    client: Option<&str>,
    request_token: Option<&str>,
) -> crate::control::ControlResponse {
    if payload.get("auth").is_none() {
        if let Some(token) = request_token {
            payload["auth"] = serde_json::Value::String(token.to_string());
        }
    }
    handle_request_value(payload, control_state, client)
}

fn ide_session_token(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv("X-Trust-Ide-Session"))
        .map(|header| header.value.as_str().to_string())
}

fn ide_write_enabled(_control_state: &ControlState) -> bool {
    true
}

fn ide_error_response(error: IdeError) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut payload = json!({ "ok": false, "error": error.to_string() });
    if let Some(version) = error.current_version() {
        payload["current_version"] = json!(version);
    }
    Response::from_string(payload.to_string())
        .with_status_code(StatusCode(error.status_code()))
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
}

fn format_web_url(listen: &str, tls: bool) -> String {
    let host = listen.split(':').next().unwrap_or("localhost");
    let port = listen.rsplit(':').next().unwrap_or("8080");
    let host = if host == "0.0.0.0" { "localhost" } else { host };
    let scheme = if tls { "https" } else { "http" };
    format!("{scheme}://{host}:{port}")
}

fn render_qr_svg(text: &str) -> Result<String, RuntimeError> {
    let code = QrCode::new(text.as_bytes())
        .map_err(|err| RuntimeError::ControlError(format!("qr: {err}").into()))?;
    let svg = code.render::<svg::Color>().min_dimensions(120, 120).build();
    Ok(svg)
}

fn parse_limit(url: &str) -> Option<u64> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == "limit" {
            return parts.next().and_then(|value| value.parse().ok());
        }
    }
    None
}

fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == key {
            let raw = parts.next().unwrap_or_default();
            return Some(decode_url_component(raw));
        }
    }
    None
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ide_task_to_snapshot(job: &IdeTaskJob) -> IdeTaskSnapshot {
    IdeTaskSnapshot {
        job_id: job.job_id,
        kind: job.kind.clone(),
        status: job.status.clone(),
        success: job.success,
        output: job.output.clone(),
        locations: parse_task_locations(job.output.as_str()),
        started_ms: job.started_ms,
        finished_ms: job.finished_ms,
    }
}

fn parse_task_locations(output: &str) -> Vec<IdeTaskLocation> {
    let mut seen = std::collections::BTreeSet::new();
    let mut locations = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        let line = line.strip_prefix("[stderr] ").unwrap_or(line);
        let Some(location) = parse_task_location_line(line) else {
            continue;
        };
        let key = format!("{}:{}:{}", location.path, location.line, location.column);
        if seen.insert(key) {
            locations.push(location);
        }
        if locations.len() >= 80 {
            break;
        }
    }
    locations
}

fn parse_task_location_line(line: &str) -> Option<IdeTaskLocation> {
    let marker = ".st:";
    let marker_pos = line.find(marker)?;
    let path = line[..marker_pos + marker.len() - 1].trim().to_string();
    if path.is_empty() {
        return None;
    }

    let mut rest = &line[marker_pos + marker.len()..];
    let line_end = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(rest.len());
    if line_end == 0 {
        return None;
    }
    let line_number = rest[..line_end].parse::<u32>().ok()?;
    rest = &rest[line_end..];

    let mut column_number = 1_u32;
    if let Some(after_colon) = rest.strip_prefix(':') {
        let column_end = after_colon
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(after_colon.len());
        if column_end > 0 {
            column_number = after_colon[..column_end].parse::<u32>().unwrap_or(1);
            rest = &after_colon[column_end..];
        } else {
            rest = after_colon;
        }
    }

    let message = rest
        .trim_start_matches(':')
        .trim_start_matches('-')
        .trim()
        .to_string();
    Some(IdeTaskLocation {
        path,
        line: line_number,
        column: column_number,
        message,
    })
}

fn ide_task_snapshot(
    store: Arc<Mutex<HashMap<u64, IdeTaskJob>>>,
    job_id: u64,
) -> Option<IdeTaskSnapshot> {
    let guard = store.lock().ok()?;
    guard.get(&job_id).map(ide_task_to_snapshot)
}

fn ide_task_append_output(store: &Arc<Mutex<HashMap<u64, IdeTaskJob>>>, job_id: u64, chunk: &str) {
    const MAX_OUTPUT_BYTES: usize = 512 * 1024;
    if let Ok(mut guard) = store.lock() {
        if let Some(job) = guard.get_mut(&job_id) {
            job.output.push_str(chunk);
            if job.output.len() > MAX_OUTPUT_BYTES {
                let excess = job.output.len() - MAX_OUTPUT_BYTES;
                job.output.drain(..excess);
            }
        }
    }
}

fn ide_task_finish(
    store: &Arc<Mutex<HashMap<u64, IdeTaskJob>>>,
    job_id: u64,
    success: bool,
    tail_message: &str,
) {
    if let Ok(mut guard) = store.lock() {
        if let Some(job) = guard.get_mut(&job_id) {
            job.status = "completed".to_string();
            job.success = Some(success);
            job.finished_ms = Some(now_ms());
            if !tail_message.is_empty() {
                job.output.push_str(tail_message);
            }
        }
    }
}

fn stream_pipe_to_job<R: std::io::Read + Send + 'static>(
    reader: R,
    prefix: &'static str,
    store: Arc<Mutex<HashMap<u64, IdeTaskJob>>>,
    job_id: u64,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let buffered = BufReader::new(reader);
        for line in buffered.lines().map_while(Result::ok) {
            ide_task_append_output(&store, job_id, format!("{prefix}{line}\n").as_str());
        }
    })
}

fn start_ide_task_job(
    kind: &str,
    project_root: PathBuf,
    store: Arc<Mutex<HashMap<u64, IdeTaskJob>>>,
    seq: Arc<AtomicU64>,
) -> IdeTaskSnapshot {
    let job_id = seq.fetch_add(1, Ordering::Relaxed);
    let job = IdeTaskJob {
        job_id,
        kind: kind.to_string(),
        status: "running".to_string(),
        success: None,
        output: String::new(),
        started_ms: now_ms(),
        finished_ms: None,
    };
    if let Ok(mut guard) = store.lock() {
        guard.insert(job_id, job.clone());
    }

    let kind_text = kind.to_string();
    let store_bg = store.clone();
    thread::spawn(move || {
        let exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(err) => {
                ide_task_finish(
                    &store_bg,
                    job_id,
                    false,
                    format!("[error] cannot resolve runtime executable: {err}\n").as_str(),
                );
                return;
            }
        };
        let mut command = Command::new(exe);
        if kind_text == "build" {
            command
                .arg("build")
                .arg("--project")
                .arg(project_root.as_os_str())
                .arg("--sources")
                .arg("src");
        } else if kind_text == "validate" {
            command
                .arg("validate")
                .arg("--project")
                .arg(project_root.as_os_str());
        } else {
            command
                .arg("test")
                .arg("--project")
                .arg(project_root.as_os_str());
        }
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(project_root);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                ide_task_finish(
                    &store_bg,
                    job_id,
                    false,
                    format!("[error] failed to start task: {err}\n").as_str(),
                );
                return;
            }
        };

        let stdout_handle = child
            .stdout
            .take()
            .map(|stdout| stream_pipe_to_job(stdout, "", store_bg.clone(), job_id));
        let stderr_handle = child
            .stderr
            .take()
            .map(|stderr| stream_pipe_to_job(stderr, "[stderr] ", store_bg.clone(), job_id));

        let wait_result = child.wait();
        if let Some(handle) = stdout_handle {
            let _ = handle.join();
        }
        if let Some(handle) = stderr_handle {
            let _ = handle.join();
        }

        match wait_result {
            Ok(status) => {
                let success = status.success();
                let tail = if success {
                    "\n[done] task completed successfully\n".to_string()
                } else {
                    format!(
                        "\n[failed] task exited with code {}\n",
                        status.code().unwrap_or(-1)
                    )
                };
                ide_task_finish(&store_bg, job_id, success, tail.as_str());
            }
            Err(err) => {
                ide_task_finish(
                    &store_bg,
                    job_id,
                    false,
                    format!("\n[error] failed waiting for task: {err}\n").as_str(),
                );
            }
        }
    });

    ide_task_to_snapshot(&job)
}

fn decode_url_component(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        match byte {
            b'%' => {
                let hi = chars.next().unwrap_or(b'0');
                let lo = chars.next().unwrap_or(b'0');
                let hex = [hi, lo];
                if let Ok(text) = std::str::from_utf8(&hex) {
                    if let Ok(value) = u8::from_str_radix(text, 16) {
                        bytes.push(value);
                    }
                }
            }
            b'+' => bytes.push(b' '),
            _ => bytes.push(byte),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn parse_probe_response(text: &str) -> Value {
    let value: Value = serde_json::from_str(text).unwrap_or_else(|_| json!({}));
    let ok = value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        let error = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unreachable");
        return json!({ "ok": false, "error": error });
    }
    let result = value.get("result").cloned().unwrap_or_else(|| json!({}));
    let name = result
        .get("plc_name")
        .or_else(|| result.get("resource"))
        .and_then(|v| v.as_str())
        .unwrap_or("PLC");
    let state = result
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("online");
    json!({ "ok": true, "name": name, "state": state })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_task_location_line_extracts_st_coordinates() {
        let parsed = parse_task_location_line("main.st:18:7 expected ';'")
            .expect("expected parsed location");
        assert_eq!(parsed.path, "main.st");
        assert_eq!(parsed.line, 18);
        assert_eq!(parsed.column, 7);
        assert!(parsed.message.contains("expected"));
    }

    #[test]
    fn parse_task_locations_deduplicates_repeated_hits() {
        let output = "\
[stderr] main.st:4:2 bad token\n\
[stderr] main.st:4:2 bad token\n\
[stderr] folder/aux.st:9:1 unresolved symbol\n";
        let parsed = parse_task_locations(output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].path, "main.st");
        assert_eq!(parsed[1].path, "folder/aux.st");
    }
}
