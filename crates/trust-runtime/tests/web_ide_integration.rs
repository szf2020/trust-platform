use std::collections::VecDeque;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use serde_json::{json, Value};
use smol_str::SmolStr;
use trust_runtime::config::{ControlMode, WebAuthMode, WebConfig};
use trust_runtime::control::{ControlState, HmiRuntimeDescriptor, SourceFile, SourceRegistry};
use trust_runtime::debug::DebugVariableHandles;
use trust_runtime::error::RuntimeError;
use trust_runtime::harness::TestHarness;
use trust_runtime::metrics::RuntimeMetrics;
use trust_runtime::scheduler::{ResourceCommand, ResourceControl, StdClock};
use trust_runtime::settings::{
    BaseSettings, DiscoverySettings, MeshSettings, RuntimeSettings, SimulationSettings, WebSettings,
};
use trust_runtime::watchdog::{FaultPolicy, RetainMode, WatchdogPolicy};
use trust_runtime::web::start_web_server;

fn runtime_settings() -> RuntimeSettings {
    RuntimeSettings::new(
        BaseSettings {
            log_level: SmolStr::new("info"),
            watchdog: WatchdogPolicy::default(),
            fault_policy: FaultPolicy::SafeHalt,
            retain_mode: RetainMode::None,
            retain_save_interval: None,
        },
        WebSettings {
            enabled: true,
            listen: SmolStr::new("127.0.0.1:0"),
            auth: SmolStr::new("local"),
            tls: false,
        },
        DiscoverySettings {
            enabled: false,
            service_name: SmolStr::new("truST"),
            advertise: false,
            interfaces: Vec::new(),
        },
        MeshSettings {
            enabled: false,
            listen: SmolStr::new("127.0.0.1:0"),
            tls: false,
            auth_token: None,
            publish: Vec::new(),
            subscribe: IndexMap::new(),
        },
        SimulationSettings {
            enabled: false,
            time_scale: 1,
            mode_label: SmolStr::new("production"),
            warning: SmolStr::new(""),
        },
    )
}

fn control_state(source: &str, mode: ControlMode, auth_token: Option<&str>) -> Arc<ControlState> {
    let mut harness = TestHarness::from_source(source).expect("build test harness");
    let debug = harness.runtime_mut().enable_debug();
    harness.cycle();

    let snapshot = trust_runtime::debug::DebugSnapshot {
        storage: harness.runtime().storage().clone(),
        now: harness.runtime().current_time(),
    };

    let (resource, cmd_rx) = ResourceControl::stub(StdClock::new());
    thread::spawn(move || {
        while let Ok(command) = cmd_rx.recv() {
            match command {
                ResourceCommand::ReloadBytecode { respond_to, .. } => {
                    let _ = respond_to
                        .send(Err(RuntimeError::ControlError(SmolStr::new("unsupported"))));
                }
                ResourceCommand::MeshSnapshot { respond_to, .. } => {
                    let _ = respond_to.send(IndexMap::new());
                }
                ResourceCommand::Snapshot { respond_to } => {
                    let _ = respond_to.send(snapshot.clone());
                }
                _ => {}
            }
        }
    });

    let sources = SourceRegistry::new(vec![SourceFile {
        id: 1,
        path: PathBuf::from("main.st"),
        text: source.to_string(),
    }]);
    let hmi_descriptor = Arc::new(Mutex::new(HmiRuntimeDescriptor::from_sources(
        None, &sources,
    )));

    Arc::new(ControlState {
        debug,
        resource,
        metadata: Arc::new(Mutex::new(harness.runtime().metadata_snapshot())),
        sources,
        io_snapshot: Arc::new(Mutex::new(None)),
        pending_restart: Arc::new(Mutex::new(None)),
        auth_token: Arc::new(Mutex::new(auth_token.map(SmolStr::new))),
        control_requires_auth: auth_token.is_some(),
        control_mode: Arc::new(Mutex::new(mode)),
        audit_tx: None,
        metrics: Arc::new(Mutex::new(RuntimeMetrics::default())),
        events: Arc::new(Mutex::new(VecDeque::new())),
        settings: Arc::new(Mutex::new(runtime_settings())),
        project_root: None,
        resource_name: SmolStr::new("RESOURCE"),
        io_health: Arc::new(Mutex::new(Vec::new())),
        debug_enabled: Arc::new(AtomicBool::new(true)),
        debug_variables: Arc::new(Mutex::new(DebugVariableHandles::new())),
        hmi_live: Arc::new(Mutex::new(trust_runtime::hmi::HmiLiveState::default())),
        hmi_descriptor,
        historian: None,
        pairing: None,
    })
}

fn reserve_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

fn start_test_server_with_root(
    state: Arc<ControlState>,
    project_root: Option<PathBuf>,
    auth: WebAuthMode,
) -> String {
    let port = reserve_loopback_port();
    let listen = format!("127.0.0.1:{port}");
    let config = WebConfig {
        enabled: true,
        listen: SmolStr::new(listen.clone()),
        auth,
        tls: false,
    };
    let _server =
        start_web_server(&config, state, None, None, project_root, None).expect("start server");
    let base = format!("http://{listen}");
    wait_for_server(&base);
    base
}

fn start_test_server(state: Arc<ControlState>, project_root: PathBuf, auth: WebAuthMode) -> String {
    start_test_server_with_root(state, Some(project_root), auth)
}

fn wait_for_server(base: &str) {
    for _ in 0..100 {
        if ureq::get(&format!("{base}/ide")).call().is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("web server did not become reachable at {base}");
}

fn request_json(
    method: &str,
    url: &str,
    payload: Option<Value>,
    headers: &[(&str, &str)],
) -> (u16, Value) {
    let mut request = match method {
        "GET" => ureq::get(url),
        "POST" => ureq::post(url),
        other => panic!("unsupported method {other}"),
    };
    for (name, value) in headers {
        request = request.set(name, value);
    }

    let result = match (method, payload) {
        ("POST", Some(body)) => request
            .set("Content-Type", "application/json")
            .send_string(&body.to_string()),
        ("POST", None) => request.send_string("{}"),
        ("GET", _) => request.call(),
        _ => unreachable!(),
    };

    match result {
        Ok(response) => {
            let status = response.status();
            let body = response.into_string().expect("read success body");
            let json = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
            (status, json)
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().expect("read error body");
            let json = serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
            (status, json)
        }
        Err(err) => panic!("request failed: {err}"),
    }
}

fn make_project(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("trust-runtime-web-ide-{name}-{stamp}"));
    std::fs::create_dir_all(&root).expect("create project dir");
    std::fs::write(
        root.join("main.st"),
        "PROGRAM Main\nVAR\nCounter : INT := 1;\nEND_VAR\nEND_PROGRAM\n",
    )
    .expect("write source");
    root
}

fn source_fixture() -> &'static str {
    "PROGRAM Main\nEND_PROGRAM\n"
}

fn p95(samples: &[Duration]) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * 0.95).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len().saturating_sub(1))]
}

fn position_for(text: &str, needle: &str) -> (u32, u32) {
    let byte_index = text.find(needle).expect("needle should exist");
    let before = &text[..byte_index];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let character = before
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count() as u32)
        .unwrap_or_else(|| before.chars().count() as u32);
    (line, character)
}

#[test]
fn web_ide_shell_serves_local_hashed_assets_without_cdn_dependency() {
    let project = make_project("shell-assets");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let shell = ureq::get(&format!("{base}/ide"))
        .call()
        .expect("fetch ide shell")
        .into_string()
        .expect("read ide shell");
    assert!(
        shell.contains("/ide/ide.js"),
        "ide shell must reference external ide.js"
    );
    assert!(
        shell.contains("/ide/ide.css"),
        "ide shell must reference external ide.css"
    );
    assert!(
        !shell.contains("esm.sh/"),
        "ide shell must not depend on esm.sh at runtime"
    );

    let ide_js = ureq::get(&format!("{base}/ide/ide.js"))
        .call()
        .expect("fetch ide.js")
        .into_string()
        .expect("read ide.js");
    assert!(
        ide_js.contains("/ide/assets/ide-monaco.20260215.js"),
        "ide.js must reference local bundled monaco asset"
    );

    let ide_css = ureq::get(&format!("{base}/ide/ide.css"))
        .call()
        .expect("fetch ide.css")
        .into_string()
        .expect("read ide.css");
    assert!(ide_css.len() > 500, "ide.css looks unexpectedly small");

    let bundle = ureq::get(&format!("{base}/ide/assets/ide-monaco.20260215.js"))
        .call()
        .expect("fetch ide bundle")
        .into_string()
        .expect("read ide bundle");
    assert!(
        bundle.len() > 100_000,
        "local monaco bundle looks unexpectedly small"
    );

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_auth_and_session_contract() {
    let project = make_project("auth");
    let state = control_state(source_fixture(), ControlMode::Debug, Some("secret-token"));
    let base = start_test_server(state, project.clone(), WebAuthMode::Token);

    let (status, unauthorized) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    assert_eq!(status, 401);
    assert_eq!(
        unauthorized.get("error").and_then(Value::as_str),
        Some("unauthorized")
    );

    let (status, session_body) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[("X-Trust-Token", "secret-token")],
    );
    assert_eq!(status, 200);
    let session_token = session_body
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (status, files) = request_json(
        "GET",
        &format!("{base}/api/ide/files"),
        None,
        &[("X-Trust-Ide-Session", session_token)],
    );
    assert_eq!(status, 200);
    assert!(files
        .get("result")
        .and_then(|v| v.get("files"))
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("main.st"))));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_project_open_endpoint_supports_no_bundle_startup() {
    let project = make_project("project-open");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server_with_root(state, None, WebAuthMode::Local);

    let (_, session_body) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let session_token = session_body
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (status, initial_project) = request_json(
        "GET",
        &format!("{base}/api/ide/project"),
        None,
        &[("X-Trust-Ide-Session", session_token)],
    );
    assert_eq!(status, 200);
    assert!(initial_project
        .get("result")
        .and_then(|v| v.get("active_project"))
        .is_some_and(Value::is_null));

    let (status, opened_project) = request_json(
        "POST",
        &format!("{base}/api/ide/project/open"),
        Some(json!({ "path": project.display().to_string() })),
        &[("X-Trust-Ide-Session", session_token)],
    );
    assert_eq!(status, 200);
    assert!(opened_project
        .get("result")
        .and_then(|v| v.get("active_project"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.contains("project-open")));

    let (status, files) = request_json(
        "GET",
        &format!("{base}/api/ide/files"),
        None,
        &[("X-Trust-Ide-Session", session_token)],
    );
    assert_eq!(status, 200);
    assert!(files
        .get("result")
        .and_then(|v| v.get("files"))
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some("main.st"))));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_collaborative_conflict_contract() {
    let project = make_project("conflict");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, s1) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let (_, s2) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token_a = s1
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("token a");
    let token_b = s2
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("token b");

    let (_, a_open) = request_json(
        "GET",
        &format!("{base}/api/ide/file?path=main.st"),
        None,
        &[("X-Trust-Ide-Session", token_a)],
    );
    let (_, b_open) = request_json(
        "GET",
        &format!("{base}/api/ide/file?path=main.st"),
        None,
        &[("X-Trust-Ide-Session", token_b)],
    );

    let version_a = a_open
        .get("result")
        .and_then(|v| v.get("version"))
        .and_then(Value::as_u64)
        .expect("version a");
    let version_b = b_open
        .get("result")
        .and_then(|v| v.get("version"))
        .and_then(Value::as_u64)
        .expect("version b");
    assert_eq!(version_a, version_b);

    let (status, first_write) = request_json(
        "POST",
        &format!("{base}/api/ide/file"),
        Some(json!({
            "path": "main.st",
            "expected_version": version_a,
            "content": "PROGRAM Main\nVAR\nCounter : INT := 2;\nEND_VAR\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", token_a)],
    );
    assert_eq!(status, 200);
    let current_version = first_write
        .get("result")
        .and_then(|v| v.get("version"))
        .and_then(Value::as_u64)
        .expect("current version");

    let (status, conflict) = request_json(
        "POST",
        &format!("{base}/api/ide/file"),
        Some(json!({
            "path": "main.st",
            "expected_version": version_b,
            "content": "PROGRAM Main\nVAR\nCounter : INT := 9;\nEND_VAR\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", token_b)],
    );
    assert_eq!(status, 409);
    assert_eq!(
        conflict.get("current_version").and_then(Value::as_u64),
        Some(current_version)
    );

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_viewer_sessions_are_read_only_and_editor_sessions_can_write() {
    let project = make_project("mode");
    let state = control_state(source_fixture(), ControlMode::Production, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, caps) = request_json("GET", &format!("{base}/api/ide/capabilities"), None, &[]);
    assert_eq!(
        caps.get("result")
            .and_then(|v| v.get("mode"))
            .and_then(Value::as_str),
        Some("authoring")
    );

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "viewer" })),
        &[],
    );
    let viewer_token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (_, opened) = request_json(
        "GET",
        &format!("{base}/api/ide/file?path=main.st"),
        None,
        &[("X-Trust-Ide-Session", viewer_token)],
    );
    assert_eq!(
        opened
            .get("result")
            .and_then(|v| v.get("read_only"))
            .and_then(Value::as_bool),
        Some(true)
    );
    let version = opened
        .get("result")
        .and_then(|v| v.get("version"))
        .and_then(Value::as_u64)
        .expect("version");

    let (status, denied) = request_json(
        "POST",
        &format!("{base}/api/ide/file"),
        Some(json!({
            "path": "main.st",
            "expected_version": version,
            "content": "PROGRAM Main\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", viewer_token)],
    );
    assert_eq!(status, 403);
    assert!(denied
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("session role does not allow edits")));

    let (_, editor_session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let editor_token = editor_session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("editor token");

    let (status, written) = request_json(
        "POST",
        &format!("{base}/api/ide/file"),
        Some(json!({
            "path": "main.st",
            "expected_version": version,
            "content": "PROGRAM Main\nVAR\nCounter : INT := 7;\nEND_VAR\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", editor_token)],
    );
    assert_eq!(status, 200);
    assert!(written
        .get("result")
        .and_then(|v| v.get("version"))
        .and_then(Value::as_u64)
        .is_some_and(|next| next > version));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_latency_and_resource_budget_contract() {
    let project = make_project("budget");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, caps) = request_json("GET", &format!("{base}/api/ide/capabilities"), None, &[]);
    let max_file_bytes = caps
        .get("result")
        .and_then(|v| v.get("limits"))
        .and_then(|v| v.get("max_file_bytes"))
        .and_then(Value::as_u64)
        .expect("max_file_bytes");

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (_, opened) = request_json(
        "GET",
        &format!("{base}/api/ide/file?path=main.st"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    let mut version = opened
        .get("result")
        .and_then(|v| v.get("version"))
        .and_then(Value::as_u64)
        .expect("initial version");

    let runs: u32 = 60;
    let mut total = Duration::ZERO;
    let mut max = Duration::ZERO;
    for idx in 0..runs {
        let started = Instant::now();
        let (status, result) = request_json(
            "POST",
            &format!("{base}/api/ide/file"),
            Some(json!({
                "path": "main.st",
                "expected_version": version,
                "content": format!(
                    "PROGRAM Main\\nVAR\\nCounter : INT := {};\\nEND_VAR\\nEND_PROGRAM\\n",
                    idx
                )
            })),
            &[("X-Trust-Ide-Session", token)],
        );
        let elapsed = started.elapsed();
        total += elapsed;
        max = max.max(elapsed);
        assert_eq!(status, 200);
        version = result
            .get("result")
            .and_then(|v| v.get("version"))
            .and_then(Value::as_u64)
            .expect("next version");
    }

    let avg = total / runs;
    assert!(
        max < Duration::from_millis(250),
        "max latency {:?} exceeded budget",
        max
    );
    assert!(
        avg < Duration::from_millis(50),
        "avg latency {:?} exceeded budget",
        avg
    );

    let too_large = "X".repeat(max_file_bytes as usize + 1);
    let (status, body) = request_json(
        "POST",
        &format!("{base}/api/ide/file"),
        Some(json!({
            "path": "main.st",
            "expected_version": version,
            "content": too_large
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 413);
    assert!(body
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("exceeds limit")));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_reference_performance_gates_contract() {
    let project = make_project("perf-gates");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let mut boot_samples = Vec::new();
    for _ in 0..8 {
        let started = Instant::now();
        let _ = request_json("GET", &format!("{base}/api/ide/capabilities"), None, &[]);
        let (_, session) = request_json(
            "POST",
            &format!("{base}/api/ide/session"),
            Some(json!({ "role": "editor" })),
            &[],
        );
        let token = session
            .get("result")
            .and_then(|v| v.get("token"))
            .and_then(Value::as_str)
            .expect("session token");
        let _ = request_json(
            "GET",
            &format!("{base}/api/ide/files"),
            None,
            &[("X-Trust-Ide-Session", token)],
        );
        let _ = request_json(
            "GET",
            &format!("{base}/api/ide/file?path=main.st"),
            None,
            &[("X-Trust-Ide-Session", token)],
        );
        boot_samples.push(started.elapsed());
    }

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let doc_text =
        "PROGRAM Main\nVAR\nCounter : INT;\nEND_VAR\n\nCounter := Counter + 1;\nEND_PROGRAM\n";

    let mut completion_samples = Vec::new();
    let mut hover_samples = Vec::new();
    let mut diagnostics_samples = Vec::new();
    let mut search_samples = Vec::new();

    for _ in 0..35 {
        let started = Instant::now();
        let (status, _) = request_json(
            "POST",
            &format!("{base}/api/ide/completion"),
            Some(json!({
                "path": "main.st",
                "content": doc_text,
                "position": { "line": 5, "character": 7 },
                "limit": 30
            })),
            &[("X-Trust-Ide-Session", token)],
        );
        assert_eq!(status, 200);
        completion_samples.push(started.elapsed());

        let started = Instant::now();
        let (status, _) = request_json(
            "POST",
            &format!("{base}/api/ide/hover"),
            Some(json!({
                "path": "main.st",
                "content": doc_text,
                "position": { "line": 5, "character": 2 }
            })),
            &[("X-Trust-Ide-Session", token)],
        );
        assert_eq!(status, 200);
        hover_samples.push(started.elapsed());

        let started = Instant::now();
        let (status, _) = request_json(
            "POST",
            &format!("{base}/api/ide/diagnostics"),
            Some(json!({
                "path": "main.st",
                "content": doc_text
            })),
            &[("X-Trust-Ide-Session", token)],
        );
        assert_eq!(status, 200);
        diagnostics_samples.push(started.elapsed());

        let started = Instant::now();
        let (status, _) = request_json(
            "GET",
            &format!("{base}/api/ide/search?q=Counter&include=**/*.st&limit=40"),
            None,
            &[("X-Trust-Ide-Session", token)],
        );
        assert_eq!(status, 200);
        search_samples.push(started.elapsed());
    }

    let two_k_line_content = {
        let mut lines = String::new();
        lines.push_str("PROGRAM Main\nVAR\nCounter : INT;\nEND_VAR\n");
        for idx in 0..2000 {
            lines.push_str(&format!("Counter := Counter + {idx};\n"));
        }
        lines.push_str("END_PROGRAM\n");
        lines
    };
    let mut typing_freeze_max = Duration::ZERO;
    for _ in 0..25 {
        let started = Instant::now();
        let (status, _) = request_json(
            "POST",
            &format!("{base}/api/ide/completion"),
            Some(json!({
                "path": "main.st",
                "content": two_k_line_content,
                "position": { "line": 1200, "character": 12 },
                "limit": 20
            })),
            &[("X-Trust-Ide-Session", token)],
        );
        assert_eq!(status, 200);
        typing_freeze_max = typing_freeze_max.max(started.elapsed());
    }

    assert!(
        p95(&boot_samples) <= Duration::from_millis(2500),
        "boot-to-ready p95 exceeded 2.5s budget: {:?}",
        p95(&boot_samples)
    );
    assert!(
        p95(&completion_samples) <= Duration::from_millis(150),
        "completion p95 exceeded 150ms budget: {:?}",
        p95(&completion_samples)
    );
    assert!(
        p95(&hover_samples) <= Duration::from_millis(150),
        "hover p95 exceeded 150ms budget: {:?}",
        p95(&hover_samples)
    );
    assert!(
        p95(&diagnostics_samples) <= Duration::from_millis(300),
        "diagnostics p95 exceeded 300ms budget: {:?}",
        p95(&diagnostics_samples)
    );
    assert!(
        p95(&search_samples) <= Duration::from_millis(400),
        "workspace search p95 exceeded 400ms budget: {:?}",
        p95(&search_samples)
    );
    assert!(
        typing_freeze_max <= Duration::from_millis(800),
        "typing freeze max exceeded 800ms budget: {:?}",
        typing_freeze_max
    );

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_analysis_and_health_endpoints_contract() {
    let project = make_project("analysis-endpoints");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (status, diagnostics) = request_json(
        "POST",
        &format!("{base}/api/ide/diagnostics"),
        Some(json!({
            "path": "main.st",
            "content": "PROGRAM Main\nVAR\nCounter : INT;\nEND_VAR\n\nCounter := UnknownSymbol + 1;\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(diagnostics
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| {
            item.get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("UnknownSymbol"))
        })));

    let (status, hover) = request_json(
        "POST",
        &format!("{base}/api/ide/hover"),
        Some(json!({
            "path": "main.st",
            "content": "PROGRAM Main\nVAR\nCounter : INT;\nEND_VAR\n\nCounter := Counter + 1;\nEND_PROGRAM\n",
            "position": { "line": 5, "character": 2 }
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(hover
        .get("result")
        .and_then(|value| value.get("contents"))
        .and_then(Value::as_str)
        .is_some_and(|value| value.contains("Counter")));

    let (status, completion) = request_json(
        "POST",
        &format!("{base}/api/ide/completion"),
        Some(json!({
            "path": "main.st",
            "content": "PRO\nPROGRAM Main\nEND_PROGRAM\n",
            "position": { "line": 0, "character": 3 },
            "limit": 20
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(completion
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|item| { item.get("label").and_then(Value::as_str) == Some("PROGRAM") })));

    let (status, health) = request_json(
        "GET",
        &format!("{base}/api/ide/health"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(health
        .get("result")
        .and_then(|value| value.get("active_sessions"))
        .and_then(Value::as_u64)
        .is_some_and(|count| count >= 1));
    assert!(health
        .get("result")
        .and_then(|value| value.get("tracked_documents"))
        .and_then(Value::as_u64)
        .is_some_and(|count| count >= 1));
    assert_eq!(
        health
            .get("result")
            .and_then(|value| value.get("frontend_telemetry"))
            .and_then(|value| value.get("bootstrap_failures"))
            .and_then(Value::as_u64),
        Some(0)
    );

    let (status, telemetry) = request_json(
        "POST",
        &format!("{base}/api/ide/frontend-telemetry"),
        Some(json!({
            "bootstrap_failures": 1,
            "analysis_timeouts": 2,
            "worker_restarts": 3,
            "autosave_failures": 4
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        telemetry
            .get("result")
            .and_then(|value| value.get("bootstrap_failures"))
            .and_then(Value::as_u64),
        Some(1)
    );

    let (status, health_after_telemetry) = request_json(
        "GET",
        &format!("{base}/api/ide/health"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        health_after_telemetry
            .get("result")
            .and_then(|value| value.get("frontend_telemetry"))
            .and_then(|value| value.get("analysis_timeouts"))
            .and_then(Value::as_u64),
        Some(2)
    );

    let (status, presence_model) = request_json(
        "GET",
        &format!("{base}/api/ide/presence-model"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        presence_model
            .get("result")
            .and_then(|value| value.get("mode"))
            .and_then(Value::as_str),
        Some("out_of_scope_phase_1")
    );

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_format_endpoint_contract() {
    let project = make_project("format-endpoint");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (status, formatted) = request_json(
        "POST",
        &format!("{base}/api/ide/format"),
        Some(json!({
            "path": "main.st",
            "content": "PROGRAM Main\nVAR\nA:INT;\nEND_VAR\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        formatted
            .get("result")
            .and_then(|v| v.get("path"))
            .and_then(Value::as_str),
        Some("main.st")
    );
    assert_eq!(
        formatted
            .get("result")
            .and_then(|v| v.get("changed"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(formatted
        .get("result")
        .and_then(|v| v.get("content"))
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("  VAR") && content.contains("    A:INT;")));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_tree_and_filesystem_endpoints_contract() {
    let project = make_project("tree-fs");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (status, tree) = request_json(
        "GET",
        &format!("{base}/api/ide/tree"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(tree
        .get("result")
        .and_then(|v| v.get("tree"))
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty()));

    let (status, created_dir) = request_json(
        "POST",
        &format!("{base}/api/ide/fs/create"),
        Some(json!({ "path": "folder_a", "kind": "directory" })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        created_dir
            .get("result")
            .and_then(|v| v.get("kind"))
            .and_then(Value::as_str),
        Some("directory")
    );

    let (status, created_file) = request_json(
        "POST",
        &format!("{base}/api/ide/fs/create"),
        Some(json!({
            "path": "folder_a/extra.st",
            "kind": "file",
            "content": "PROGRAM Extra\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        created_file
            .get("result")
            .and_then(|v| v.get("path"))
            .and_then(Value::as_str),
        Some("folder_a/extra.st")
    );

    let (status, create_conflict) = request_json(
        "POST",
        &format!("{base}/api/ide/fs/create"),
        Some(json!({
            "path": "folder_a/extra.st",
            "kind": "file",
            "content": "PROGRAM Extra\nEND_PROGRAM\n"
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 409);
    assert!(create_conflict
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("already exists")));

    let (status, renamed_file) = request_json(
        "POST",
        &format!("{base}/api/ide/fs/rename"),
        Some(json!({
            "path": "folder_a/extra.st",
            "new_path": "folder_a/renamed_extra.st"
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        renamed_file
            .get("result")
            .and_then(|v| v.get("path"))
            .and_then(Value::as_str),
        Some("folder_a/renamed_extra.st")
    );

    let (status, open_renamed) = request_json(
        "GET",
        &format!("{base}/api/ide/file?path=folder_a/renamed_extra.st"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(open_renamed
        .get("result")
        .and_then(|v| v.get("content"))
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("PROGRAM Extra")));

    let (status, _) = request_json(
        "POST",
        &format!("{base}/api/ide/fs/delete"),
        Some(json!({ "path": "folder_a/renamed_extra.st" })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);

    let (status, files_after_delete) = request_json(
        "GET",
        &format!("{base}/api/ide/files"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(!files_after_delete
        .get("result")
        .and_then(|v| v.get("files"))
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|item| item.as_str() == Some("folder_a/renamed_extra.st"))));

    let (status, audit) = request_json(
        "GET",
        &format!("{base}/api/ide/fs/audit?limit=20"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(audit
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| items.len() >= 3));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_security_and_path_traversal_contract() {
    let project = make_project("security");
    let state = control_state(source_fixture(), ControlMode::Production, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, viewer_session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "viewer" })),
        &[],
    );
    let viewer_token = viewer_session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("viewer token");

    let (status, viewer_write) = request_json(
        "POST",
        &format!("{base}/api/ide/fs/create"),
        Some(json!({ "path": "viewer_blocked.st", "kind": "file" })),
        &[("X-Trust-Ide-Session", viewer_token)],
    );
    assert_eq!(status, 403);
    assert!(viewer_write
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("session role does not allow edits")));

    let (status, _viewer_build) = request_json(
        "POST",
        &format!("{base}/api/ide/build"),
        Some(json!({})),
        &[("X-Trust-Ide-Session", viewer_token)],
    );
    assert_eq!(status, 403);

    let (status, _viewer_validate) = request_json(
        "POST",
        &format!("{base}/api/ide/validate"),
        Some(json!({})),
        &[("X-Trust-Ide-Session", viewer_token)],
    );
    assert_eq!(status, 403);

    let (_, editor_session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let editor_token = editor_session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("editor token");

    let traversal_cases = [
        (
            "POST",
            format!("{base}/api/ide/fs/create"),
            json!({ "path": "../escape.st", "kind": "file" }),
        ),
        (
            "POST",
            format!("{base}/api/ide/fs/rename"),
            json!({ "path": "main.st", "new_path": "../escaped.st" }),
        ),
        (
            "POST",
            format!("{base}/api/ide/fs/delete"),
            json!({ "path": "../main.st" }),
        ),
        (
            "POST",
            format!("{base}/api/ide/file"),
            json!({
                "path": "../main.st",
                "expected_version": 1,
                "content": "PROGRAM Main\nEND_PROGRAM\n"
            }),
        ),
    ];

    for (method, url, payload) in traversal_cases {
        let (status, body) = request_json(
            method,
            &url,
            Some(payload),
            &[("X-Trust-Ide-Session", editor_token)],
        );
        assert_eq!(status, 403);
        assert!(body
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("workspace path escapes project root")));
    }

    let (status, invalid_glob) = request_json(
        "GET",
        &format!("{base}/api/ide/search?q=Main&include=[&limit=20"),
        None,
        &[("X-Trust-Ide-Session", editor_token)],
    );
    assert_eq!(status, 400);
    assert!(invalid_glob
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("invalid include glob pattern")));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_navigation_search_and_rename_endpoints_contract() {
    let project = make_project("nav-rename");
    std::fs::write(
        project.join("types.st"),
        "TYPE\n    MyType : STRUCT\n        value : INT;\n    END_STRUCT;\nEND_TYPE\n",
    )
    .expect("write types");
    std::fs::write(
        project.join("main.st"),
        "PROGRAM Main\nVAR\nitem : MyType;\nCounter : INT;\nEND_VAR\nCounter := Counter + 1;\nEND_PROGRAM\n",
    )
    .expect("write main");

    let main_source = std::fs::read_to_string(project.join("main.st")).expect("read main");
    let (my_type_line, my_type_char) = position_for(&main_source, "MyType");
    let (counter_line, counter_char) = position_for(&main_source, "Counter := Counter");

    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    let (status, definition) = request_json(
        "POST",
        &format!("{base}/api/ide/definition"),
        Some(json!({
            "path": "main.st",
            "position": { "line": my_type_line, "character": my_type_char }
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert_eq!(
        definition
            .get("result")
            .and_then(|v| v.get("path"))
            .and_then(Value::as_str),
        Some("types.st")
    );

    let (status, references) = request_json(
        "POST",
        &format!("{base}/api/ide/references"),
        Some(json!({
            "path": "main.st",
            "position": { "line": counter_line, "character": counter_char },
            "include_declaration": true
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(references
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| items.len() >= 2));

    let (status, rename) = request_json(
        "POST",
        &format!("{base}/api/ide/rename"),
        Some(json!({
            "path": "main.st",
            "position": { "line": my_type_line, "character": my_type_char },
            "new_name": "MyTypeRenamed"
        })),
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(rename
        .get("result")
        .and_then(|v| v.get("edit_count"))
        .and_then(Value::as_u64)
        .is_some_and(|count| count >= 2));

    let (status, search) = request_json(
        "GET",
        &format!("{base}/api/ide/search?q=MyTypeRenamed&limit=20"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(search
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty()));

    let (status, search_scoped) = request_json(
        "GET",
        &format!("{base}/api/ide/search?q=MyTypeRenamed&include=types.st&limit=20"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(search_scoped
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty()
            && items
                .iter()
                .all(|item| item.get("path").and_then(Value::as_str) == Some("types.st"))));

    let (status, search_excluded) = request_json(
        "GET",
        &format!("{base}/api/ide/search?q=MyTypeRenamed&exclude=types.st&limit=20"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(!search_excluded
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|item| item.get("path").and_then(Value::as_str) == Some("types.st"))));

    let (status, symbols) = request_json(
        "GET",
        &format!("{base}/api/ide/symbols?q=Main&limit=20"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(symbols
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|item| {
            item.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("Main"))
        })));

    let (status, file_symbols) = request_json(
        "GET",
        &format!("{base}/api/ide/symbols?q=MyTypeRenamed&path=types.st&limit=20"),
        None,
        &[("X-Trust-Ide-Session", token)],
    );
    assert_eq!(status, 200);
    assert!(file_symbols
        .get("result")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .all(|item| { item.get("path").and_then(Value::as_str) == Some("types.st") })));

    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn web_ide_build_test_and_validate_task_endpoints_contract() {
    let project = make_project("task-endpoints");
    let state = control_state(source_fixture(), ControlMode::Debug, None);
    let base = start_test_server(state, project.clone(), WebAuthMode::Local);

    let (_, session) = request_json(
        "POST",
        &format!("{base}/api/ide/session"),
        Some(json!({ "role": "editor" })),
        &[],
    );
    let token = session
        .get("result")
        .and_then(|v| v.get("token"))
        .and_then(Value::as_str)
        .expect("session token");

    for kind in ["build", "test", "validate"] {
        let (status, task_start) = request_json(
            "POST",
            &format!("{base}/api/ide/{kind}"),
            Some(json!({})),
            &[("X-Trust-Ide-Session", token)],
        );
        assert_eq!(status, 200, "failed to start {kind} job");
        let job_id = task_start
            .get("result")
            .and_then(|v| v.get("job_id"))
            .and_then(Value::as_u64)
            .expect("job id");

        let started = Instant::now();
        let mut done = false;
        while started.elapsed() < Duration::from_secs(40) {
            let (status, task) = request_json(
                "GET",
                &format!("{base}/api/ide/task?id={job_id}"),
                None,
                &[("X-Trust-Ide-Session", token)],
            );
            assert_eq!(status, 200);
            let state_text = task
                .get("result")
                .and_then(|v| v.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if state_text == "completed" {
                done = true;
                assert!(task
                    .get("result")
                    .and_then(|v| v.get("output"))
                    .and_then(Value::as_str)
                    .is_some());
                assert!(task
                    .get("result")
                    .and_then(|v| v.get("locations"))
                    .and_then(Value::as_array)
                    .is_some());
                break;
            }
            thread::sleep(Duration::from_millis(150));
        }
        assert!(done, "{kind} task endpoint did not complete in time");
    }

    let _ = std::fs::remove_dir_all(project);
}
