//! Terminal UI for runtime monitoring and control.

#![allow(missing_docs)]

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Sparkline, Wrap},
    Terminal,
};

use crate::bundle::detect_bundle_path;
use crate::bundle_builder::build_program_stbc;
use crate::config::RuntimeBundle;
use crate::control::ControlEndpoint;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::json;

const COLOR_TEAL: Color = Color::Rgb(0, 168, 150);
const COLOR_GREEN: Color = Color::Rgb(46, 204, 113);
const COLOR_AMBER: Color = Color::Rgb(243, 156, 18);
const COLOR_RED: Color = Color::Rgb(231, 76, 60);
const COLOR_INFO: Color = Color::Rgb(142, 142, 147);
const COLOR_YELLOW: Color = Color::Rgb(245, 196, 66);
const COLOR_CYAN: Color = Color::Rgb(64, 212, 255);
const COLOR_MAGENTA: Color = Color::Rgb(191, 90, 242);
const COLOR_PROMPT_BG: Color = Color::Rgb(24, 24, 24);

#[derive(Default, Clone)]
struct UiData {
    status: Option<StatusSnapshot>,
    tasks: Vec<TaskSnapshot>,
    io: Vec<IoEntry>,
    events: Vec<EventSnapshot>,
    settings: Option<SettingsSnapshot>,
}

#[derive(Default, Clone)]
struct StatusSnapshot {
    state: String,
    fault: String,
    resource: String,
    uptime_ms: u64,
    cycle_min: f64,
    cycle_avg: f64,
    cycle_max: f64,
    cycle_last: f64,
    overruns: u64,
    faults: u64,
    drivers: Vec<DriverSnapshot>,
    debug_enabled: bool,
    control_mode: String,
}

#[derive(Default, Clone)]
struct TaskSnapshot {
    name: String,
    last_ms: f64,
    avg_ms: f64,
    max_ms: f64,
    overruns: u64,
}

#[derive(Default, Clone)]
struct DriverSnapshot {
    name: String,
    status: String,
    error: Option<String>,
}

#[derive(Default, Clone)]
struct IoEntry {
    name: String,
    address: String,
    value: String,
    direction: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Info,
    Warn,
    Fault,
}

#[derive(Clone)]
struct EventSnapshot {
    label: String,
    kind: EventKind,
    timestamp: Option<String>,
    message: String,
}

impl Default for EventSnapshot {
    fn default() -> Self {
        Self {
            label: String::new(),
            kind: EventKind::Info,
            timestamp: None,
            message: String::new(),
        }
    }
}

#[derive(Default, Clone)]
struct SettingsSnapshot {
    log_level: String,
    watchdog_enabled: bool,
    watchdog_timeout_ms: i64,
    watchdog_action: String,
    fault_policy: String,
    retain_mode: String,
    retain_save_interval_ms: Option<i64>,
    web_listen: String,
    web_auth: String,
    discovery_enabled: bool,
    mesh_enabled: bool,
    mesh_publish: Vec<String>,
    mesh_subscribe: Vec<(String, String)>,
    control_mode: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmAction {
    RestartWarm,
    RestartCold,
    Shutdown,
    ExitConsole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelKind {
    Cycle,
    Io,
    Status,
    Events,
    Tasks,
    Watch,
}

impl PanelKind {
    fn title(self) -> &'static str {
        match self {
            PanelKind::Cycle => "Cycle Time",
            PanelKind::Io => "I/O",
            PanelKind::Status => "Status",
            PanelKind::Events => "Events",
            PanelKind::Tasks => "Tasks",
            PanelKind::Watch => "Watch",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "cycle" => Some(Self::Cycle),
            "io" => Some(Self::Io),
            "status" => Some(Self::Status),
            "events" => Some(Self::Events),
            "tasks" => Some(Self::Tasks),
            "watch" => Some(Self::Watch),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptMode {
    Normal,
    SettingsSelect,
    SettingsValue(SettingKey),
    IoSelect(IoActionKind),
    IoValueSelect,
    ConfirmAction(ConfirmAction),
    Menu(MenuKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Io,
    Control,
    Access,
    Linking,
    Log,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IoActionKind {
    Read,
    Set,
    Force,
    Unforce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingKey {
    PlcName,
    CycleInterval,
    LogLevel,
    ControlMode,
    WebListen,
    WebAuth,
    DiscoveryEnabled,
    MeshEnabled,
}

#[derive(Debug, Clone)]
struct PromptLine {
    segments: Vec<(String, Style)>,
}

impl PromptLine {
    fn plain(text: impl Into<String>, style: Style) -> Self {
        Self {
            segments: vec![(text.into(), style)],
        }
    }

    fn from_segments<T: Into<String>>(segments: Vec<(T, Style)>) -> Self {
        Self {
            segments: segments
                .into_iter()
                .map(|(text, style)| (text.into(), style))
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
struct PromptState {
    active: bool,
    input: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    output: Vec<PromptLine>,
    mode: PromptMode,
    showing_suggestions: bool,
    suggestions: Vec<CommandHelp>,
    suggestion_index: usize,
}

impl PromptState {
    fn new() -> Self {
        Self {
            active: false,
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            output: Vec::new(),
            mode: PromptMode::Normal,
            showing_suggestions: false,
            suggestions: Vec::new(),
            suggestion_index: 0,
        }
    }

    fn activate_with(&mut self, text: &str) {
        self.active = true;
        self.input.clear();
        self.input.push_str(text);
        self.cursor = self.input.len();
        self.history_index = None;
    }

    fn deactivate(&mut self) {
        self.active = false;
        self.cursor = 0;
        self.history_index = None;
    }

    fn set_output(&mut self, lines: Vec<PromptLine>) {
        self.output = lines;
        self.showing_suggestions = false;
    }

    fn clear_output(&mut self) {
        self.output.clear();
        self.showing_suggestions = false;
    }

    fn set_suggestions_list(&mut self, suggestions: Vec<CommandHelp>) {
        self.suggestions = suggestions;
        self.suggestion_index = 0;
        self.showing_suggestions = true;
        self.output = suggestion_lines(&self.suggestions, Some(self.suggestion_index));
    }

    fn clear_suggestions(&mut self) {
        if self.showing_suggestions {
            self.output.clear();
        }
        self.showing_suggestions = false;
        self.suggestions.clear();
        self.suggestion_index = 0;
    }

    fn move_suggestion(&mut self, delta: i32) {
        if self.suggestions.is_empty() {
            return;
        }
        let len = self.suggestions.len() as i32;
        let mut next = self.suggestion_index as i32 + delta;
        if next < 0 {
            next = len - 1;
        } else if next >= len {
            next = 0;
        }
        self.suggestion_index = next as usize;
        self.output = suggestion_lines(&self.suggestions, Some(self.suggestion_index));
    }

    fn selected_suggestion(&self) -> Option<CommandHelp> {
        self.suggestions.get(self.suggestion_index).copied()
    }

    fn push_history(&mut self, entry: String) {
        if !entry.trim().is_empty() {
            self.history.push(entry);
        }
        self.history_index = None;
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_index {
            None => Some(self.history.len().saturating_sub(1)),
            Some(idx) if idx > 0 => Some(idx - 1),
            Some(idx) => Some(idx),
        };
        if let Some(idx) = next {
            self.history_index = Some(idx);
            self.input = self.history[idx].clone();
            self.cursor = self.input.len();
        }
    }

    fn history_next(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_index {
            None => None,
            Some(idx) if idx + 1 < self.history.len() => Some(idx + 1),
            Some(_) => None,
        };
        self.history_index = next;
        match next {
            Some(idx) => {
                self.input = self.history[idx].clone();
                self.cursor = self.input.len();
            }
            None => {
                self.input.clear();
                self.cursor = 0;
            }
        }
    }
}

struct UiState {
    data: UiData,
    pending_confirm: Option<ConfirmAction>,
    beginner_mode: bool,
    debug_controls: bool,
    prompt: PromptState,
    layout: Vec<PanelKind>,
    focus: Option<PanelKind>,
    panel_page: usize,
    settings_index: usize,
    menu_index: usize,
    io_index: usize,
    io_value_index: usize,
    io_pending_address: Option<String>,
    io_pending_action: Option<IoActionKind>,
    cycle_history: VecDeque<u64>,
    watch_list: Vec<String>,
    watch_values: Vec<(String, String)>,
    forced_io: HashSet<String>,
    alerts: VecDeque<PromptLine>,
    seen_events: HashSet<String>,
    connected: bool,
    bundle_root: Option<PathBuf>,
}

pub fn run_ui(
    bundle: Option<PathBuf>,
    endpoint: Option<String>,
    token: Option<String>,
    refresh_ms: u64,
    no_input: bool,
    beginner: bool,
) -> anyhow::Result<()> {
    let (endpoint, auth_token, bundle_root) = resolve_endpoint(bundle, endpoint, token)?;
    let console_config = bundle_root
        .as_ref()
        .map(|root| load_console_config(root))
        .unwrap_or_default();
    let layout = console_config.layout.unwrap_or_else(|| {
        vec![
            PanelKind::Cycle,
            PanelKind::Io,
            PanelKind::Status,
            PanelKind::Events,
        ]
    });
    let refresh_ms = if refresh_ms == 250 {
        console_config.refresh_ms.unwrap_or(refresh_ms)
    } else {
        refresh_ms
    };
    let mut state = UiState {
        data: UiData::default(),
        pending_confirm: None,
        beginner_mode: beginner,
        debug_controls: !beginner,
        prompt: PromptState::new(),
        layout,
        focus: None,
        panel_page: 0,
        settings_index: 0,
        menu_index: 0,
        io_index: 0,
        io_value_index: 0,
        io_pending_address: None,
        io_pending_action: None,
        cycle_history: VecDeque::with_capacity(120),
        watch_list: Vec::new(),
        watch_values: Vec::new(),
        forced_io: HashSet::new(),
        alerts: VecDeque::with_capacity(6),
        seen_events: HashSet::new(),
        connected: true,
        bundle_root,
    };
    let mut client = ControlClient::connect(endpoint.clone(), auth_token.clone())?;
    let mut last_refresh = Instant::now();
    let refresh = StdDuration::from_millis(refresh_ms);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = (|| {
        loop {
            if last_refresh.elapsed() >= refresh {
                match fetch_data(&mut client) {
                    Ok(data) => {
                        if !state.connected {
                            push_alert(
                                &mut state,
                                "CONNECTED Control restored.",
                                Style::default().fg(COLOR_GREEN),
                            );
                        }
                        state.connected = true;
                        state.data = data;
                        if let Some(status) = state.data.status.as_ref() {
                            state.debug_controls = !state.beginner_mode && status.debug_enabled;
                        }
                        update_cycle_history(&mut state);
                        update_watch_values(&mut client, &mut state);
                        update_event_alerts(&mut state);
                    }
                    Err(_) => {
                        if state.connected {
                            push_alert(
                                &mut state,
                                "DISCONNECTED Reconnecting...",
                                Style::default().fg(COLOR_AMBER),
                            );
                        }
                        state.connected = false;
                        if let Ok(new_client) =
                            ControlClient::connect(endpoint.clone(), auth_token.clone())
                        {
                            client = new_client;
                        }
                    }
                }
                last_refresh = Instant::now();
            }

            terminal.draw(|frame| render_ui(frame.size(), frame, &state, no_input))?;

            if event::poll(StdDuration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if handle_key(key, &mut client, &mut state, no_input)? {
                        break;
                    }
                }
            }
        }
        Ok(())
    })();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn resolve_endpoint(
    bundle: Option<PathBuf>,
    endpoint: Option<String>,
    token: Option<String>,
) -> anyhow::Result<(ControlEndpoint, Option<String>, Option<PathBuf>)> {
    let mut auth = token.or_else(|| std::env::var("TRUST_CTL_TOKEN").ok());
    if let Some(endpoint) = endpoint {
        return Ok((ControlEndpoint::parse(&endpoint)?, auth, bundle));
    }
    let bundle_path = detect_bundle_path(bundle).map_err(anyhow::Error::from)?;
    let bundle = RuntimeBundle::load(bundle_path.clone())?;
    if auth.is_none() {
        auth = bundle
            .runtime
            .control_auth_token
            .as_ref()
            .map(|value| value.to_string());
    }
    Ok((
        ControlEndpoint::parse(bundle.runtime.control_endpoint.as_str())?,
        auth,
        Some(bundle_path),
    ))
}

#[derive(Default)]
struct ConsoleConfig {
    layout: Option<Vec<PanelKind>>,
    refresh_ms: Option<u64>,
}

fn load_console_config(root: &Path) -> ConsoleConfig {
    let path = root.join("runtime.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return ConsoleConfig::default(),
    };
    let value: toml::Value = match text.parse() {
        Ok(value) => value,
        Err(_) => return ConsoleConfig::default(),
    };
    let console = match value.get("console") {
        Some(console) => console,
        None => return ConsoleConfig::default(),
    };
    let layout = console
        .get("layout")
        .and_then(|value| value.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| entry.as_str())
                .filter_map(PanelKind::parse)
                .collect::<Vec<_>>()
        });
    let refresh_ms = console
        .get("refresh_ms")
        .and_then(|value| value.as_integer())
        .and_then(|value| u64::try_from(value).ok());
    ConsoleConfig { layout, refresh_ms }
}

fn fetch_data(client: &mut ControlClient) -> anyhow::Result<UiData> {
    let status = client.request(json!({"id": 1, "type": "status"}))?;
    let tasks = client.request(json!({"id": 2, "type": "tasks.stats"}))?;
    let io = client.request(json!({"id": 3, "type": "io.list"}))?;
    let events =
        client.request(json!({"id": 4, "type": "events.tail", "params": { "limit": 20 }}))?;
    let settings = client.request(json!({"id": 5, "type": "config.get"}))?;
    Ok(UiData {
        status: parse_status(&status),
        tasks: parse_tasks(&tasks),
        io: parse_io(&io),
        events: parse_events(&events),
        settings: parse_settings(&settings),
    })
}

fn parse_status(response: &serde_json::Value) -> Option<StatusSnapshot> {
    let result = response.get("result")?;
    Some(StatusSnapshot {
        state: result.get("state")?.as_str()?.to_string(),
        fault: result
            .get("fault")
            .and_then(|v| v.as_str())
            .unwrap_or("none")
            .to_string(),
        resource: result
            .get("resource")
            .and_then(|v| v.as_str())
            .unwrap_or("resource")
            .to_string(),
        uptime_ms: result
            .get("uptime_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cycle_min: result
            .get("metrics")
            .and_then(|m| m.get("cycle_ms"))
            .and_then(|v| v.get("min"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        cycle_avg: result
            .get("metrics")
            .and_then(|m| m.get("cycle_ms"))
            .and_then(|v| v.get("avg"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        cycle_max: result
            .get("metrics")
            .and_then(|m| m.get("cycle_ms"))
            .and_then(|v| v.get("max"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        cycle_last: result
            .get("metrics")
            .and_then(|m| m.get("cycle_ms"))
            .and_then(|v| v.get("last"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        overruns: result
            .get("metrics")
            .and_then(|m| m.get("overruns"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        faults: result
            .get("metrics")
            .and_then(|m| m.get("faults"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        drivers: result
            .get("io_drivers")
            .and_then(|v| v.as_array())
            .map(|drivers| {
                drivers
                    .iter()
                    .map(|entry| DriverSnapshot {
                        name: entry
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("driver")
                            .to_string(),
                        status: entry
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        error: entry
                            .get("error")
                            .and_then(|v| v.as_str())
                            .map(|value| value.to_string()),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        debug_enabled: result
            .get("debug_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        control_mode: result
            .get("control_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("production")
            .to_string(),
    })
}

fn parse_tasks(response: &serde_json::Value) -> Vec<TaskSnapshot> {
    let mut out = Vec::new();
    let tasks = response
        .get("result")
        .and_then(|r| r.get("tasks"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for task in tasks {
        let name = task.get("name").and_then(|v| v.as_str()).unwrap_or("task");
        out.push(TaskSnapshot {
            name: name.to_string(),
            last_ms: task.get("last_ms").and_then(|v| v.as_f64()).unwrap_or(0.0),
            avg_ms: task.get("avg_ms").and_then(|v| v.as_f64()).unwrap_or(0.0),
            max_ms: task.get("max_ms").and_then(|v| v.as_f64()).unwrap_or(0.0),
            overruns: task.get("overruns").and_then(|v| v.as_u64()).unwrap_or(0),
        });
    }
    out
}

fn parse_io(response: &serde_json::Value) -> Vec<IoEntry> {
    let mut out = Vec::new();
    let result = response.get("result");
    let add_entries =
        |entries: Option<&Vec<serde_json::Value>>, direction: &str, out: &mut Vec<IoEntry>| {
            if let Some(entries) = entries {
                for entry in entries {
                    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let address = entry.get("address").and_then(|v| v.as_str()).unwrap_or("");
                    let value = entry
                        .get("value")
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    out.push(IoEntry {
                        name: name.to_string(),
                        address: address.to_string(),
                        value,
                        direction: direction.to_string(),
                    });
                }
            }
        };
    if let Some(result) = result {
        let inputs = result.get("inputs").and_then(|v| v.as_array());
        let outputs = result.get("outputs").and_then(|v| v.as_array());
        add_entries(inputs, "IN", &mut out);
        add_entries(outputs, "OUT", &mut out);
    }
    out
}

fn parse_events(response: &serde_json::Value) -> Vec<EventSnapshot> {
    let mut out = Vec::new();
    let events = response
        .get("result")
        .and_then(|r| r.get("events"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for event in events {
        let Some(obj) = event.as_object() else {
            continue;
        };
        let kind = match obj.get("type").and_then(|v| v.as_str()).unwrap_or("info") {
            "fault" => EventKind::Fault,
            "task_overrun" => EventKind::Warn,
            _ => EventKind::Info,
        };
        let (timestamp, message, label) = format_event_label(obj);
        if message.is_empty() {
            continue;
        }
        out.push(EventSnapshot {
            label,
            kind,
            timestamp,
            message,
        });
    }
    out
}

fn parse_settings(response: &serde_json::Value) -> Option<SettingsSnapshot> {
    let result = response.get("result")?;
    Some(SettingsSnapshot {
        log_level: result.get("log.level")?.as_str()?.to_string(),
        watchdog_enabled: result
            .get("watchdog.enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        watchdog_timeout_ms: result
            .get("watchdog.timeout_ms")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        watchdog_action: result
            .get("watchdog.action")
            .and_then(|v| v.as_str())
            .unwrap_or("halt")
            .to_string(),
        fault_policy: result
            .get("fault.policy")
            .and_then(|v| v.as_str())
            .unwrap_or("halt")
            .to_string(),
        retain_mode: result
            .get("retain.mode")
            .and_then(|v| v.as_str())
            .unwrap_or("none")
            .to_string(),
        retain_save_interval_ms: result
            .get("retain.save_interval_ms")
            .and_then(|v| v.as_i64()),
        web_listen: result
            .get("web.listen")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0:8080")
            .to_string(),
        web_auth: result
            .get("web.auth")
            .and_then(|v| v.as_str())
            .unwrap_or("local")
            .to_string(),
        discovery_enabled: result
            .get("discovery.enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        mesh_enabled: result
            .get("mesh.enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        mesh_publish: result
            .get("mesh.publish")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|v| v.to_string())
                    .collect()
            })
            .unwrap_or_default(),
        mesh_subscribe: result
            .get("mesh.subscribe")
            .and_then(|v| v.as_object())
            .map(|map| {
                map.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        control_mode: result
            .get("control.mode")
            .and_then(|v| v.as_str())
            .unwrap_or("production")
            .to_string(),
    })
}

fn handle_key(
    key: KeyEvent,
    client: &mut ControlClient,
    state: &mut UiState,
    no_input: bool,
) -> anyhow::Result<bool> {
    if no_input && key.code == KeyCode::Char('q') {
        return Ok(true);
    }

    if state.prompt.active {
        return handle_prompt_key(key, client, state);
    }

    if no_input {
        if key.code == KeyCode::Char('/') {
            state.prompt.set_output(vec![PromptLine::plain(
                "Read-only mode.",
                Style::default().fg(COLOR_INFO),
            )]);
        }
        return Ok(false);
    }

    if let Some(confirm) = state.pending_confirm.take() {
        return handle_confirm(confirm, key, client);
    }

    if key.code == KeyCode::Char('/') {
        state.prompt.activate_with("/");
        state
            .prompt
            .set_suggestions_list(command_suggestions(state, None));
        return Ok(false);
    }

    if key.code == KeyCode::Tab {
        advance_panel_page(state);
        return Ok(false);
    }

    let action = match key.code {
        KeyCode::Char('p') | KeyCode::Char('P') => Some("pause"),
        KeyCode::Char('r') | KeyCode::Char('R') => Some("resume"),
        KeyCode::Char('s') | KeyCode::Char('S') => Some("step_in"),
        KeyCode::Char('o') | KeyCode::Char('O') => Some("step_over"),
        KeyCode::Char('u') | KeyCode::Char('U') => Some("step_out"),
        KeyCode::Char('w') | KeyCode::Char('W') => {
            state.pending_confirm = Some(ConfirmAction::RestartWarm);
            return Ok(false);
        }
        KeyCode::Char('c') | KeyCode::Char('C') => {
            state.pending_confirm = Some(ConfirmAction::RestartCold);
            return Ok(false);
        }
        KeyCode::Char('x') | KeyCode::Char('X') => {
            state.pending_confirm = Some(ConfirmAction::Shutdown);
            return Ok(false);
        }
        KeyCode::Char('q') | KeyCode::Char('Q') => return Ok(true),
        _ => None,
    };

    if let Some(action) = action {
        if matches!(
            action,
            "pause" | "resume" | "step_in" | "step_over" | "step_out"
        ) && !state.debug_controls
        {
            state.prompt.set_output(vec![PromptLine::plain(
                "Debug controls disabled.",
                Style::default().fg(COLOR_AMBER),
            )]);
            return Ok(false);
        }
        let request = match action {
            "pause" => json!({"id": 1, "type": "pause"}),
            "resume" => json!({"id": 1, "type": "resume"}),
            "step_in" => json!({"id": 1, "type": "step_in"}),
            "step_over" => json!({"id": 1, "type": "step_over"}),
            "step_out" => json!({"id": 1, "type": "step_out"}),
            _ => json!({"id": 1, "type": "status"}),
        };
        let _ = client.request(request);
    }
    Ok(false)
}

fn handle_prompt_key(
    key: KeyEvent,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            let mode = state.prompt.mode;
            state.prompt.deactivate();
            state.prompt.clear_suggestions();
            match mode {
                PromptMode::IoSelect(_) => {
                    open_menu(MenuKind::Io, state);
                }
                PromptMode::IoValueSelect => {
                    if let Some(action) = state.io_pending_action {
                        open_io_select(action, state);
                    } else {
                        open_menu(MenuKind::Io, state);
                    }
                }
                PromptMode::Menu(_) | PromptMode::SettingsSelect => {
                    state.prompt.clear_output();
                    state.prompt.mode = PromptMode::Normal;
                }
                _ => {
                    state.prompt.clear_output();
                    state.prompt.mode = PromptMode::Normal;
                }
            }
            return Ok(false);
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.prompt.deactivate();
            state.prompt.mode = PromptMode::Normal;
            state.prompt.clear_suggestions();
            return Ok(false);
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.prompt.mode = PromptMode::ConfirmAction(ConfirmAction::ExitConsole);
            state.prompt.set_output(vec![PromptLine::plain(
                "Exit console? [y/N]",
                Style::default().fg(COLOR_AMBER),
            )]);
            state.prompt.input.clear();
            state.prompt.cursor = 0;
            return Ok(false);
        }
        KeyCode::Enter => {
            if state.prompt.showing_suggestions
                && state.prompt.mode == PromptMode::Normal
                && !state.prompt.input.trim().contains(' ')
            {
                if let Some(selected) = state.prompt.selected_suggestion() {
                    let cmd = format!("/{}", selected.cmd);
                    state.prompt.push_history(cmd.clone());
                    state.prompt.deactivate();
                    state.prompt.clear_suggestions();
                    return execute_command(&cmd, client, state);
                }
            }
            let input = state.prompt.input.trim().to_string();
            state.prompt.push_history(input.clone());
            state.prompt.deactivate();
            state.prompt.clear_suggestions();
            return handle_prompt_submit(&input, client, state);
        }
        KeyCode::Backspace => {
            if state.prompt.cursor > 0 {
                state.prompt.cursor -= 1;
                state.prompt.input.remove(state.prompt.cursor);
            }
        }
        KeyCode::Left => {
            if state.prompt.cursor > 0 {
                state.prompt.cursor -= 1;
            }
        }
        KeyCode::Right => {
            if state.prompt.cursor < state.prompt.input.len() {
                state.prompt.cursor += 1;
            }
        }
        KeyCode::Up => {
            if state.prompt.showing_suggestions {
                state.prompt.move_suggestion(-1);
                return Ok(false);
            }
            if state.prompt.mode == PromptMode::SettingsSelect {
                move_settings_selection(state, -1);
                return Ok(false);
            }
            if let PromptMode::Menu(kind) = state.prompt.mode {
                move_menu_selection(state, kind, -1);
                return Ok(false);
            }
            if let PromptMode::IoSelect(action) = state.prompt.mode {
                move_io_selection(state, action, -1);
                return Ok(false);
            }
            if state.prompt.mode == PromptMode::IoValueSelect {
                move_io_value_selection(state, -1);
                return Ok(false);
            }
            state.prompt.history_prev();
        }
        KeyCode::Down => {
            if state.prompt.showing_suggestions {
                state.prompt.move_suggestion(1);
                return Ok(false);
            }
            if state.prompt.mode == PromptMode::SettingsSelect {
                move_settings_selection(state, 1);
                return Ok(false);
            }
            if let PromptMode::Menu(kind) = state.prompt.mode {
                move_menu_selection(state, kind, 1);
                return Ok(false);
            }
            if let PromptMode::IoSelect(action) = state.prompt.mode {
                move_io_selection(state, action, 1);
                return Ok(false);
            }
            if state.prompt.mode == PromptMode::IoValueSelect {
                move_io_value_selection(state, 1);
                return Ok(false);
            }
            state.prompt.history_next();
        }
        KeyCode::Char(ch) => {
            state.prompt.input.insert(state.prompt.cursor, ch);
            state.prompt.cursor += 1;
        }
        _ => {}
    }
    update_command_suggestions(state);
    Ok(false)
}

fn handle_prompt_submit(
    input: &str,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    let trimmed = input.trim();
    if trimmed.is_empty() && state.prompt.mode == PromptMode::Normal {
        return Ok(false);
    }

    match state.prompt.mode {
        PromptMode::SettingsSelect => return handle_settings_select(trimmed, client, state),
        PromptMode::SettingsValue(key) => {
            return handle_settings_value(trimmed, key, client, state)
        }
        PromptMode::Menu(kind) => return handle_menu_select(trimmed, kind, client, state),
        PromptMode::IoSelect(action) => return handle_io_select(trimmed, action, client, state),
        PromptMode::IoValueSelect => return handle_io_value_select(trimmed, client, state),
        PromptMode::ConfirmAction(action) => {
            return handle_prompt_confirm(trimmed, action, client, state)
        }
        PromptMode::Normal => {}
    }

    execute_command(trimmed, client, state)
}

fn handle_prompt_confirm(
    input: &str,
    action: ConfirmAction,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => match action {
            ConfirmAction::ExitConsole => return Ok(true),
            ConfirmAction::Shutdown => {
                let _ = client.request(json!({"id": 1, "type": "shutdown"}));
                state.prompt.set_output(vec![PromptLine::plain(
                    "Shutdown requested.",
                    Style::default().fg(COLOR_GREEN),
                )]);
            }
            ConfirmAction::RestartCold => {
                let _ = client
                    .request(json!({"id": 1, "type": "restart", "params": { "mode": "cold" }}));
                state.prompt.set_output(vec![PromptLine::plain(
                    "Restarting (cold)...",
                    Style::default().fg(COLOR_GREEN),
                )]);
            }
            ConfirmAction::RestartWarm => {
                let _ = client
                    .request(json!({"id": 1, "type": "restart", "params": { "mode": "warm" }}));
                state.prompt.set_output(vec![PromptLine::plain(
                    "Restarting (warm)...",
                    Style::default().fg(COLOR_GREEN),
                )]);
            }
        },
        _ => {
            state.prompt.set_output(vec![PromptLine::plain(
                "Cancelled.",
                Style::default().fg(COLOR_INFO),
            )]);
        }
    }
    state.prompt.mode = PromptMode::Normal;
    Ok(false)
}

fn handle_settings_select(
    input: &str,
    _client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    let choice = input.trim();
    let entries = settings_menu_entries(state);
    let selected = if choice.is_empty() {
        Some(state.settings_index)
    } else if let Ok(num) = choice.parse::<usize>() {
        if num == 0 {
            Some(entries.len().saturating_sub(1))
        } else {
            num.checked_sub(1)
        }
    } else {
        None
    };
    let Some(selected) = selected else {
        state.prompt.set_output(vec![PromptLine::plain(
            "Invalid choice.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    };
    if selected >= entries.len() {
        state.prompt.set_output(vec![PromptLine::plain(
            "Invalid choice.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    }
    let entry = &entries[selected];
    if entry.key.is_none() {
        state.prompt.clear_output();
        state.prompt.mode = PromptMode::Normal;
        return Ok(false);
    }
    let key = entry.key.unwrap();
    let current_value = normalize_setting_input(key, &entry.value);
    state.prompt.mode = PromptMode::SettingsValue(key);
    state.prompt.set_output(vec![
        PromptLine::plain(format_setting_key(key), header_style()),
        PromptLine::from_segments(vec![
            seg("Current: ", label_style()),
            seg(entry.value.clone(), value_style()),
        ]),
        PromptLine::plain(
            "Enter new value (Esc to cancel).",
            Style::default().fg(COLOR_INFO),
        ),
    ]);
    state.prompt.activate_with(&current_value);
    Ok(false)
}

fn handle_settings_value(
    input: &str,
    key: SettingKey,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    let value = input.trim();
    if value.is_empty() {
        state.prompt.set_output(vec![PromptLine::plain(
            "Value required.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    }
    let result = apply_setting(key, value, client, state)?;
    let mut lines = Vec::new();
    lines.push(PromptLine::plain(
        result.message,
        Style::default().fg(if result.ok { COLOR_GREEN } else { COLOR_RED }),
    ));
    state.prompt.set_output(lines);
    if result.restart_required {
        open_menu(MenuKind::Restart, state);
        return Ok(false);
    }
    state.prompt.mode = PromptMode::Normal;
    Ok(false)
}

fn render_ui(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, no_input: bool) {
    let mut prompt_height = (state.prompt.output.len() + state.alerts.len() + 1) as u16;
    let is_menu = matches!(
        state.prompt.mode,
        PromptMode::SettingsSelect
            | PromptMode::Menu(_)
            | PromptMode::IoSelect(_)
            | PromptMode::IoValueSelect
    );
    let max_prompt = if is_menu { 14 } else { 8 };
    if prompt_height < 3 {
        prompt_height = 3;
    }
    if prompt_height > max_prompt {
        prompt_height = max_prompt;
    }
    let min_panel_height = 8;
    if prompt_height + min_panel_height >= area.height {
        prompt_height = area.height.saturating_sub(min_panel_height).max(3);
    }
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(area.height.saturating_sub(prompt_height)),
            Constraint::Length(prompt_height),
        ])
        .split(area);
    render_panels(layout[0], frame, state);
    render_prompt(layout[1], frame, state, no_input);
}

fn render_panels(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState) {
    if let Some(panel) = state.focus {
        render_panel(area, frame, state, panel, true);
        return;
    }
    let width = area.width;
    let panels = state.layout.as_slice();
    if width >= 120 && panels.len() >= 4 {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(cols[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(cols[1]);
        render_panel(left[0], frame, state, panels[0], false);
        render_panel(right[0], frame, state, panels[1], false);
        render_panel(left[1], frame, state, panels[2], false);
        render_panel(right[1], frame, state, panels[3], false);
        return;
    }

    if width >= 80 {
        let pages = panels.len().div_ceil(2);
        let page = state.panel_page % pages.max(1);
        let start = page * 2;
        let stack = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        if let Some(panel) = panels.get(start) {
            render_panel(stack[0], frame, state, *panel, false);
        }
        if let Some(panel) = panels.get(start + 1) {
            render_panel(stack[1], frame, state, *panel, false);
        }
        return;
    }

    let panel = panels
        .get(state.panel_page % panels.len().max(1))
        .copied()
        .unwrap_or(PanelKind::Status);
    render_panel(area, frame, state, panel, false);
}

fn render_panel(
    area: Rect,
    frame: &mut ratatui::Frame<'_>,
    state: &UiState,
    panel: PanelKind,
    focused: bool,
) {
    match panel {
        PanelKind::Cycle => render_cycle_panel(area, frame, state, focused),
        PanelKind::Io => render_io_panel(area, frame, state, focused),
        PanelKind::Status => render_status_panel(area, frame, state, focused),
        PanelKind::Events => render_events_panel(area, frame, state, focused),
        PanelKind::Tasks => render_tasks_panel(area, frame, state, focused),
        PanelKind::Watch => render_watch_panel(area, frame, state, focused),
    }
}

fn render_cycle_panel(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, focused: bool) {
    let status = state.data.status.clone().unwrap_or_default();
    let block = panel_block(PanelKind::Cycle, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut data: Vec<u64> = state.cycle_history.iter().copied().collect();
    if data.is_empty() {
        data.push(0);
    }
    let spark_height = inner.height.saturating_sub(1);
    let spark_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: spark_height,
    };
    frame.render_widget(
        Sparkline::default()
            .data(&data)
            .style(Style::default().fg(COLOR_TEAL)),
        spark_area,
    );
    let stats = Line::from(vec![
        Span::styled("min ", label_style()),
        Span::styled(format!("{:.1}ms  ", status.cycle_min), value_style()),
        Span::styled("avg ", label_style()),
        Span::styled(format!("{:.1}ms  ", status.cycle_avg), value_style()),
        Span::styled("max ", label_style()),
        Span::styled(format!("{:.1}ms  ", status.cycle_max), value_style()),
        Span::styled("last ", label_style()),
        Span::styled(format!("{:.1}ms", status.cycle_last), value_style()),
    ]);
    let stats_area = Rect {
        x: inner.x,
        y: inner.y + spark_height,
        width: inner.width,
        height: 1,
    };
    frame.render_widget(Paragraph::new(stats), stats_area);
}

fn render_io_panel(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, focused: bool) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("{:<4}", "DIR"), header_style()),
        Span::raw(" "),
        Span::styled(format!("{:<12}", "NAME"), header_style()),
        Span::raw(" "),
        Span::styled(format!("{:<8}", "ADDR"), header_style()),
        Span::raw(" "),
        Span::styled(format!("{:<10}", "VALUE"), header_style()),
        Span::raw(" "),
        Span::styled("F", header_style()),
    ]));
    for entry in state
        .data
        .io
        .iter()
        .take(area.height.saturating_sub(3) as usize)
    {
        let name = if entry.name.is_empty() {
            "-".to_string()
        } else {
            entry.name.clone()
        };
        let forced = if state.forced_io.contains(&entry.address) {
            "F"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<4}", entry.direction), label_style()),
            Span::raw(" "),
            Span::styled(format!("{:<12}", name), value_style()),
            Span::raw(" "),
            Span::styled(format!("{:<8}", entry.address), value_style()),
            Span::raw(" "),
            Span::styled(format!("{:<10}", entry.value), value_style()),
            Span::raw(" "),
            Span::styled(forced, Style::default().fg(COLOR_MAGENTA)),
        ]));
    }
    let block = panel_block(PanelKind::Io, focused);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_status_panel(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, focused: bool) {
    let status = state.data.status.clone().unwrap_or_default();
    let settings = state.data.settings.clone().unwrap_or_default();
    let uptime = format_uptime(status.uptime_ms);
    let chip = status_chip(status.state.as_str());
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(chip.0, chip.1),
        Span::raw(" "),
        Span::styled(status.resource, value_style()),
    ]));
    lines.push(label_value_line("Uptime", &uptime, 12, value_style()));
    if let Some(driver) = status.drivers.first() {
        if driver.status.is_empty() {
            lines.push(label_value_line("I/O", &driver.name, 12, value_style()));
        } else {
            lines.push(label_value_line(
                "I/O",
                &format!("{} ({})", driver.name, driver.status),
                12,
                value_style(),
            ));
        }
        if let Some(err) = driver.error.as_ref() {
            lines.push(label_value_line(
                "I/O error",
                err,
                12,
                Style::default().fg(COLOR_RED),
            ));
        }
    } else {
        lines.push(label_value_line(
            "I/O",
            "unknown",
            12,
            Style::default().fg(COLOR_INFO),
        ));
    }
    lines.push(label_value_line(
        "Control",
        &format!(
            "{} (debug {})",
            status.control_mode,
            if status.debug_enabled { "on" } else { "off" }
        ),
        12,
        value_style(),
    ));
    let web = if settings.web_listen.is_empty() {
        "disabled".to_string()
    } else {
        format!("http://{}", settings.web_listen)
    };
    lines.push(label_value_line("Web", &web, 12, value_style()));
    if !status.fault.is_empty() && status.fault != "none" {
        lines.push(label_value_line(
            "Fault",
            &status.fault,
            12,
            Style::default().fg(COLOR_RED),
        ));
    }
    if status.overruns > 0 {
        lines.push(label_value_line(
            "Overruns",
            &status.overruns.to_string(),
            12,
            value_style(),
        ));
    }
    if status.faults > 0 {
        lines.push(label_value_line(
            "Faults",
            &status.faults.to_string(),
            12,
            value_style(),
        ));
    }
    let watchdog = if settings.watchdog_enabled {
        format!(
            "Watchdog: {} ms ({})",
            settings.watchdog_timeout_ms, settings.watchdog_action
        )
    } else {
        "Watchdog: disabled".to_string()
    };
    lines.push(label_value_line("Watchdog", &watchdog, 12, value_style()));
    let fault_policy = if settings.fault_policy.is_empty() {
        "unknown".to_string()
    } else {
        settings.fault_policy.clone()
    };
    lines.push(label_value_line(
        "Fault policy",
        &fault_policy,
        12,
        value_style(),
    ));
    let retain = if settings.retain_mode.is_empty() {
        "none".to_string()
    } else {
        settings.retain_mode.clone()
    };
    let retain_line = match settings.retain_save_interval_ms {
        Some(ms) => format!("Retain: {retain} ({ms} ms)"),
        None => format!("Retain: {retain}"),
    };
    lines.push(label_value_line("Retain", &retain_line, 12, value_style()));
    let block = panel_block(PanelKind::Status, focused);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_events_panel(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, focused: bool) {
    let mut lines = Vec::new();
    for event in state
        .data
        .events
        .iter()
        .take(area.height.saturating_sub(2) as usize)
    {
        let (tag, tag_style, msg_style) = match event.kind {
            EventKind::Fault => (
                "[FAULT]",
                Style::default().fg(COLOR_RED).add_modifier(Modifier::BOLD),
                Style::default().fg(Color::White),
            ),
            EventKind::Warn => (
                "[WARN]",
                Style::default()
                    .fg(COLOR_AMBER)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(Color::White),
            ),
            EventKind::Info => (
                "[INFO]",
                Style::default().fg(COLOR_CYAN),
                Style::default().fg(Color::White),
            ),
        };
        let mut spans = Vec::new();
        if let Some(ts) = event.timestamp.as_ref() {
            spans.push(Span::styled(
                format!("{ts} "),
                Style::default().fg(COLOR_INFO).add_modifier(Modifier::DIM),
            ));
        }
        spans.push(Span::styled(format!("{tag} "), tag_style));
        spans.push(Span::styled(event.message.clone(), msg_style));
        lines.push(Line::from(spans));
    }
    let block = panel_block(PanelKind::Events, focused);
    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

fn render_tasks_panel(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, focused: bool) {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(format!("{:<12}", "TASK"), header_style()),
        Span::raw(" "),
        Span::styled(format!("{:>6}", "LAST"), header_style()),
        Span::raw(" "),
        Span::styled(format!("{:>6}", "AVG"), header_style()),
        Span::raw(" "),
        Span::styled(format!("{:>6}", "MAX"), header_style()),
        Span::raw(" "),
        Span::styled(format!("{:>4}", "OVR"), header_style()),
    ]));
    for task in state
        .data
        .tasks
        .iter()
        .take(area.height.saturating_sub(3) as usize)
    {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<12}", task.name), value_style()),
            Span::raw(" "),
            Span::styled(format!("{:>6.2}", task.last_ms), value_style()),
            Span::raw(" "),
            Span::styled(format!("{:>6.2}", task.avg_ms), value_style()),
            Span::raw(" "),
            Span::styled(format!("{:>6.2}", task.max_ms), value_style()),
            Span::raw(" "),
            Span::styled(format!("{:>4}", task.overruns), value_style()),
        ]));
    }
    let block = panel_block(PanelKind::Tasks, focused);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_watch_panel(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, focused: bool) {
    let mut lines = Vec::new();
    if state.watch_values.is_empty() {
        lines.push(Line::from(Span::styled(
            "No watches configured.",
            Style::default().fg(COLOR_INFO),
        )));
    } else {
        for (name, value) in state
            .watch_values
            .iter()
            .take(area.height.saturating_sub(2) as usize)
        {
            lines.push(label_value_line(name, value, 14, value_style()));
        }
    }
    let block = panel_block(PanelKind::Watch, focused);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_prompt(area: Rect, frame: &mut ratatui::Frame<'_>, state: &UiState, no_input: bool) {
    let mut lines: Vec<Line> = Vec::new();
    for alert in state.alerts.iter().take(3) {
        lines.push(prompt_line_to_line(alert));
    }
    for line in state.prompt.output.iter() {
        lines.push(prompt_line_to_line(line));
    }
    let output_height = area.height.saturating_sub(1);
    let output_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: output_height,
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), output_area);

    let prompt_area = Rect {
        x: area.x,
        y: area.y + output_height,
        width: area.width,
        height: 1,
    };
    if no_input {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Read-only mode",
                Style::default().fg(COLOR_INFO),
            ))),
            prompt_area,
        );
        return;
    }
    if state.prompt.active {
        let prompt = Line::from(vec![
            Span::styled(
                "> ",
                Style::default().fg(COLOR_TEAL).add_modifier(Modifier::BOLD),
            ),
            Span::raw(state.prompt.input.clone()),
        ]);
        frame.render_widget(
            Paragraph::new(prompt).style(Style::default().bg(COLOR_PROMPT_BG)),
            prompt_area,
        );
        frame.set_cursor(
            prompt_area.x + 2 + state.prompt.cursor as u16,
            prompt_area.y,
        );
    } else {
        let hint = Line::from(Span::styled(
            "Press / to type command",
            Style::default()
                .fg(COLOR_INFO)
                .add_modifier(Modifier::DIM)
                .bg(COLOR_PROMPT_BG),
        ));
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().bg(COLOR_PROMPT_BG)),
            prompt_area,
        );
    }
}

fn handle_confirm(
    action: ConfirmAction,
    key: KeyEvent,
    client: &mut ControlClient,
) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            let request = match action {
                ConfirmAction::RestartWarm => {
                    json!({"id": 1, "type": "restart", "params": { "mode": "warm" }})
                }
                ConfirmAction::RestartCold => {
                    json!({"id": 1, "type": "restart", "params": { "mode": "cold" }})
                }
                ConfirmAction::Shutdown => json!({"id": 1, "type": "shutdown"}),
                ConfirmAction::ExitConsole => return Ok(true),
            };
            let _ = client.request(request);
            Ok(false)
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Ok(false),
        _ => Ok(false),
    }
}

fn prompt_line_to_line(line: &PromptLine) -> Line<'_> {
    let spans = line
        .segments
        .iter()
        .map(|(text, style)| Span::styled(text.clone(), *style))
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn panel_block(kind: PanelKind, focused: bool) -> Block<'static> {
    let border_style = if focused {
        Style::default().fg(COLOR_TEAL)
    } else {
        Style::default().fg(COLOR_INFO)
    };
    Block::default()
        .title(Span::styled(
            format!(" {} ", kind.title()),
            Style::default()
                .fg(COLOR_YELLOW)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(border_style)
}

fn label_style() -> Style {
    Style::default().fg(COLOR_CYAN)
}

fn header_style() -> Style {
    Style::default()
        .fg(COLOR_YELLOW)
        .add_modifier(Modifier::BOLD)
}

fn value_style() -> Style {
    Style::default().fg(Color::White)
}

fn label_value_line(label: &str, value: &str, width: usize, value_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<width$}"), label_style()),
        Span::raw(" "),
        Span::styled(value.to_string(), value_style),
    ])
}

fn seg(text: impl Into<String>, style: Style) -> (String, Style) {
    (text.into(), style)
}

fn advance_panel_page(state: &mut UiState) {
    let len = state.layout.len().max(1);
    state.panel_page = (state.panel_page + 1) % len;
}

fn status_chip(state: &str) -> (String, Style) {
    let upper = state.trim().to_ascii_uppercase();
    let (bg, fg) = match upper.as_str() {
        "RUNNING" => (COLOR_TEAL, Color::White),
        "PAUSED" => (COLOR_AMBER, Color::Black),
        "FAULTED" => (COLOR_RED, Color::White),
        "STOPPED" => (Color::DarkGray, Color::White),
        _ => (Color::DarkGray, Color::White),
    };
    (
        format!("[{}]", upper),
        Style::default().bg(bg).fg(fg).add_modifier(Modifier::BOLD),
    )
}

fn format_uptime(uptime_ms: u64) -> String {
    let secs = uptime_ms / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn push_alert(state: &mut UiState, text: &str, style: Style) {
    if state.alerts.len() > 4 {
        state.alerts.pop_front();
    }
    state
        .alerts
        .push_back(PromptLine::plain(text.to_string(), style));
}

fn update_cycle_history(state: &mut UiState) {
    let status = match state.data.status.as_ref() {
        Some(status) => status,
        None => return,
    };
    let value = (status.cycle_last * 10.0).max(0.0).round() as u64;
    if state.cycle_history.len() >= 120 {
        state.cycle_history.pop_front();
    }
    state.cycle_history.push_back(value.max(1));
}

fn update_watch_values(client: &mut ControlClient, state: &mut UiState) {
    if state.watch_list.is_empty() {
        state.watch_values.clear();
        return;
    }
    let mut out = Vec::new();
    for name in state.watch_list.iter() {
        let response = client.request(json!({
            "id": 1,
            "type": "eval",
            "params": { "expr": name }
        }));
        match response {
            Ok(value) => {
                if let Some(result) = value.get("result").and_then(|r| r.get("value")) {
                    out.push((name.clone(), result.to_string()));
                } else if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                    out.push((name.clone(), format!("error: {err}")));
                } else {
                    out.push((name.clone(), "unknown".to_string()));
                }
            }
            Err(_) => out.push((name.clone(), "unavailable".to_string())),
        }
    }
    state.watch_values = out;
}

fn update_event_alerts(state: &mut UiState) {
    let events = state.data.events.clone();
    for event in events {
        if state.seen_events.contains(&event.label) {
            continue;
        }
        state.seen_events.insert(event.label.clone());
        match event.kind {
            EventKind::Fault => push_alert(
                state,
                &format!("[FAULT] {}", event.message),
                Style::default().fg(COLOR_RED),
            ),
            EventKind::Warn => push_alert(
                state,
                &format!("[WARN] {}", event.message),
                Style::default().fg(COLOR_AMBER),
            ),
            EventKind::Info => {}
        }
        if state.seen_events.len() > 400 {
            state.seen_events.clear();
        }
    }
}

fn format_event_label(
    obj: &serde_json::Map<String, serde_json::Value>,
) -> (Option<String>, String, String) {
    let event_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("event");
    let time_ms = obj
        .get("time_ns")
        .and_then(|v| v.as_u64())
        .map(|ns| ns / 1_000_000);
    let timestamp = time_ms.map(|ms| format!("{:>6}ms", ms));
    let message = match event_type {
        "fault" => obj
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("fault")
            .to_string(),
        "task_overrun" => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("task");
            let missed = obj.get("missed").and_then(|v| v.as_u64()).unwrap_or(0);
            format!("overrun {name} ({missed})")
        }
        "task_start" => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("task");
            format!("task start {name}")
        }
        "task_end" => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("task");
            format!("task end {name}")
        }
        "cycle_start" | "cycle_end" => String::new(),
        _ => event_type.to_string(),
    };
    let label = if let Some(ts) = timestamp.as_ref() {
        format!("{ts} {message}")
    } else {
        message.clone()
    };
    (timestamp, message, label)
}

struct SettingApplyResult {
    ok: bool,
    restart_required: bool,
    message: String,
}

fn apply_setting(
    key: SettingKey,
    value: &str,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<SettingApplyResult> {
    let mut restart_required = false;
    let mut ok = true;
    let mut message = "Saved.".to_string();

    match key {
        SettingKey::PlcName => {
            if let Some(root) = state.bundle_root.as_ref() {
                if let Err(err) = update_runtime_toml(root, "resource.name", value) {
                    ok = false;
                    message = format!("Failed: {err}");
                } else {
                    restart_required = true;
                    message = "Saved. Restart required.".to_string();
                }
            } else {
                ok = false;
                message = "Project path required.".to_string();
            }
        }
        SettingKey::CycleInterval => {
            if let Ok(ms) = value.trim().parse::<u64>() {
                if let Some(root) = state.bundle_root.as_ref() {
                    if let Err(err) =
                        update_runtime_toml(root, "resource.cycle_interval_ms", &ms.to_string())
                    {
                        ok = false;
                        message = format!("Failed: {err}");
                    } else {
                        restart_required = true;
                        message = "Saved. Restart required.".to_string();
                    }
                } else {
                    ok = false;
                    message = "Project path required.".to_string();
                }
            } else {
                ok = false;
                message = "Invalid number.".to_string();
            }
        }
        SettingKey::LogLevel => {
            let _ = client
                .request(json!({"id": 1, "type": "config.set", "params": { "log.level": value }}));
            if let Some(root) = state.bundle_root.as_ref() {
                let _ = update_runtime_toml(root, "runtime.log.level", value);
            }
        }
        SettingKey::ControlMode => {
            let _ = client.request(
                json!({"id": 1, "type": "config.set", "params": { "control.mode": value }}),
            );
            if let Some(root) = state.bundle_root.as_ref() {
                let _ = update_runtime_toml(root, "runtime.control.mode", value);
            }
            restart_required = true;
            message = "Saved. Restart required.".to_string();
        }
        SettingKey::WebListen => {
            let _ = client
                .request(json!({"id": 1, "type": "config.set", "params": { "web.listen": value }}));
            if let Some(root) = state.bundle_root.as_ref() {
                let _ = update_runtime_toml(root, "runtime.web.listen", value);
            }
            restart_required = true;
            message = "Saved. Restart required.".to_string();
        }
        SettingKey::WebAuth => {
            let _ = client
                .request(json!({"id": 1, "type": "config.set", "params": { "web.auth": value }}));
            if let Some(root) = state.bundle_root.as_ref() {
                let _ = update_runtime_toml(root, "runtime.web.auth", value);
            }
            restart_required = true;
            message = "Saved. Restart required.".to_string();
        }
        SettingKey::DiscoveryEnabled => {
            if let Some(enabled) = parse_bool_value(value) {
                let _ = client.request(json!({
                    "id": 1,
                    "type": "config.set",
                    "params": { "discovery.enabled": enabled }
                }));
                if let Some(root) = state.bundle_root.as_ref() {
                    let _ = update_runtime_toml(
                        root,
                        "runtime.discovery.enabled",
                        &enabled.to_string(),
                    );
                }
                restart_required = true;
                message = "Saved. Restart required.".to_string();
            } else {
                ok = false;
                message = "Use true/false.".to_string();
            }
        }
        SettingKey::MeshEnabled => {
            if let Some(enabled) = parse_bool_value(value) {
                let _ = client.request(
                    json!({"id": 1, "type": "config.set", "params": { "mesh.enabled": enabled }}),
                );
                if let Some(root) = state.bundle_root.as_ref() {
                    let _ = update_runtime_toml(root, "runtime.mesh.enabled", &enabled.to_string());
                }
                restart_required = true;
                message = "Saved. Restart required.".to_string();
            } else {
                ok = false;
                message = "Use true/false.".to_string();
            }
        }
    }

    Ok(SettingApplyResult {
        ok,
        restart_required,
        message,
    })
}

fn format_setting_key(key: SettingKey) -> &'static str {
    match key {
        SettingKey::PlcName => "PLC name",
        SettingKey::CycleInterval => "Cycle interval (ms)",
        SettingKey::LogLevel => "Log level",
        SettingKey::ControlMode => "Control mode",
        SettingKey::WebListen => "Web listen",
        SettingKey::WebAuth => "Web auth",
        SettingKey::DiscoveryEnabled => "Discovery enabled",
        SettingKey::MeshEnabled => "PLC Linking enabled",
    }
}

fn execute_command(
    input: &str,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(false);
    }
    if raw == "/" {
        state
            .prompt
            .set_suggestions_list(command_suggestions(state, None));
        return Ok(false);
    }
    let mut cmd = raw;
    if let Some(stripped) = cmd.strip_prefix('/') {
        cmd = stripped;
    }
    let mut parts = cmd.split_whitespace();
    let head = parts.next().unwrap_or("");
    match head {
        "s" => {
            state.prompt.set_output(status_lines(state));
            return Ok(false);
        }
        "h" => {
            state.prompt.set_output(help_lines(state));
            return Ok(false);
        }
        "q" => return Ok(true),
        "p" => {
            handle_control_command(vec!["pause"], client, state)?;
            return Ok(false);
        }
        "r" => {
            handle_control_command(vec!["resume"], client, state)?;
            return Ok(false);
        }
        _ => {}
    }

    if state.beginner_mode && !is_beginner_command(head) {
        state.prompt.set_output(vec![PromptLine::plain(
            "Beginner mode: use /help, /status, /settings, /io, /control, /info, /exit.",
            Style::default().fg(COLOR_AMBER),
        )]);
        return Ok(false);
    }

    match head {
        "help" => {
            state.prompt.set_output(help_lines(state));
        }
        "status" => {
            state.prompt.set_output(status_lines(state));
        }
        "info" => {
            state.prompt.set_output(info_lines(state));
        }
        "clear" => {
            state.prompt.clear_output();
            state.alerts.clear();
        }
        "exit" => return Ok(true),
        "settings" => {
            state.prompt.mode = PromptMode::SettingsSelect;
            state.settings_index = 0;
            state
                .prompt
                .set_output(settings_menu_lines(state, state.settings_index));
            state.prompt.activate_with("");
        }
        "io" => {
            handle_io_command(parts.collect::<Vec<_>>(), client, state)?;
        }
        "control" => {
            handle_control_command(parts.collect::<Vec<_>>(), client, state)?;
        }
        "access" => {
            handle_access_command(parts.collect::<Vec<_>>(), client, state)?;
        }
        "linking" => {
            handle_linking_command(parts.collect::<Vec<_>>(), client, state)?;
        }
        "build" => {
            handle_build_command(state)?;
        }
        "reload" => {
            handle_reload_command(client, state)?;
        }
        "watch" => {
            if let Some(name) = parts.next() {
                if !state.watch_list.iter().any(|v| v == name) {
                    state.watch_list.push(name.to_string());
                }
                state.prompt.set_output(vec![PromptLine::plain(
                    format!("Watching {name}."),
                    Style::default().fg(COLOR_GREEN),
                )]);
            }
        }
        "unwatch" => match parts.next() {
            Some("all") => {
                state.watch_list.clear();
                state.watch_values.clear();
                state.prompt.set_output(vec![PromptLine::plain(
                    "Watches cleared.",
                    Style::default().fg(COLOR_INFO),
                )]);
            }
            Some(name) => {
                state.watch_list.retain(|v| v != name);
                state.prompt.set_output(vec![PromptLine::plain(
                    format!("Stopped watching {name}."),
                    Style::default().fg(COLOR_INFO),
                )]);
            }
            None => {
                state.prompt.set_output(vec![PromptLine::plain(
                    "Usage: /unwatch <name|all>",
                    Style::default().fg(COLOR_INFO),
                )]);
            }
        },
        "log" => {
            handle_log_command(parts.collect::<Vec<_>>(), client, state)?;
        }
        "layout" => {
            handle_layout_command(parts.collect::<Vec<_>>(), state)?;
        }
        "focus" => {
            handle_focus_command(parts.collect::<Vec<_>>(), state)?;
        }
        "unfocus" => {
            state.focus = None;
            state.prompt.set_output(vec![PromptLine::plain(
                "Returned to grid view.",
                Style::default().fg(COLOR_INFO),
            )]);
        }
        _ => {
            state.prompt.set_output(vec![PromptLine::plain(
                "Unknown command. Type /help.",
                Style::default().fg(COLOR_RED),
            )]);
        }
    }
    Ok(false)
}

fn help_lines(state: &UiState) -> Vec<PromptLine> {
    suggestion_lines(&command_suggestions(state, None), None)
}

fn update_command_suggestions(state: &mut UiState) {
    if state.prompt.mode != PromptMode::Normal || !state.prompt.active {
        return;
    }
    let input = state.prompt.input.trim();
    if !input.starts_with('/') {
        state.prompt.clear_suggestions();
        return;
    }
    let query = input.trim_start_matches('/').trim();
    if query.contains(' ') {
        state.prompt.clear_suggestions();
        return;
    }
    let filter = if query.is_empty() { None } else { Some(query) };
    let suggestions = command_suggestions(state, filter);
    if suggestions.is_empty() {
        state.prompt.clear_suggestions();
        return;
    }
    state.prompt.set_suggestions_list(suggestions);
}

fn command_suggestions(state: &UiState, filter: Option<&str>) -> Vec<CommandHelp> {
    let catalog = command_catalog(state.beginner_mode);
    catalog
        .into_iter()
        .filter(|entry| {
            if let Some(filter) = filter {
                entry.cmd.starts_with(filter)
            } else {
                true
            }
        })
        .collect()
}

fn suggestion_lines(suggestions: &[CommandHelp], selected: Option<usize>) -> Vec<PromptLine> {
    let mut lines = Vec::new();
    lines.push(PromptLine::plain("Commands:", header_style()));
    if suggestions.is_empty() {
        lines.push(PromptLine::plain(
            "No matches.",
            Style::default().fg(COLOR_INFO),
        ));
        return lines;
    }
    for (idx, entry) in suggestions.iter().enumerate() {
        let is_selected = selected == Some(idx);
        if is_selected {
            let style = Style::default()
                .bg(COLOR_TEAL)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD);
            lines.push(PromptLine::from_segments(vec![
                seg(format!("/{:<8}", entry.cmd), style),
                seg(entry.desc, style),
            ]));
        } else {
            lines.push(PromptLine::from_segments(vec![
                seg(
                    format!("/{:<8}", entry.cmd),
                    Style::default().fg(COLOR_CYAN),
                ),
                seg(entry.desc, value_style()),
            ]));
        }
    }
    lines
}

#[derive(Clone, Copy, Debug)]
struct CommandHelp {
    cmd: &'static str,
    desc: &'static str,
    beginner: bool,
}

fn command_catalog(beginner: bool) -> Vec<CommandHelp> {
    let mut entries = vec![
        CommandHelp {
            cmd: "help",
            desc: "Show all commands",
            beginner: true,
        },
        CommandHelp {
            cmd: "status",
            desc: "Show runtime status",
            beginner: true,
        },
        CommandHelp {
            cmd: "settings",
            desc: "Open settings menu",
            beginner: true,
        },
        CommandHelp {
            cmd: "io",
            desc: "I/O menu (read/write/force)",
            beginner: true,
        },
        CommandHelp {
            cmd: "control",
            desc: "Pause, resume, restart",
            beginner: true,
        },
        CommandHelp {
            cmd: "info",
            desc: "Show version, uptime",
            beginner: true,
        },
        CommandHelp {
            cmd: "exit",
            desc: "Leave console",
            beginner: true,
        },
        CommandHelp {
            cmd: "access",
            desc: "Access PLC tokens",
            beginner: false,
        },
        CommandHelp {
            cmd: "linking",
            desc: "PLC Linking (mesh)",
            beginner: false,
        },
        CommandHelp {
            cmd: "watch",
            desc: "Watch variable",
            beginner: false,
        },
        CommandHelp {
            cmd: "log",
            desc: "Show/set log level",
            beginner: false,
        },
        CommandHelp {
            cmd: "build",
            desc: "Recompile sources",
            beginner: false,
        },
        CommandHelp {
            cmd: "reload",
            desc: "Reload program bytecode",
            beginner: false,
        },
        CommandHelp {
            cmd: "layout",
            desc: "Set panel layout",
            beginner: false,
        },
        CommandHelp {
            cmd: "focus",
            desc: "Focus a panel",
            beginner: false,
        },
        CommandHelp {
            cmd: "unfocus",
            desc: "Return to grid view",
            beginner: false,
        },
        CommandHelp {
            cmd: "clear",
            desc: "Clear prompt output",
            beginner: false,
        },
    ];
    if beginner {
        entries.retain(|entry| entry.beginner);
    }
    entries
}

fn is_beginner_command(head: &str) -> bool {
    matches!(
        head,
        "help" | "status" | "settings" | "io" | "control" | "info" | "exit"
    )
}

fn status_lines(state: &UiState) -> Vec<PromptLine> {
    let status = state.data.status.clone().unwrap_or_default();
    let uptime = format_uptime(status.uptime_ms);
    let chip = status_chip(status.state.as_str());
    let line = PromptLine::from_segments(vec![
        seg(chip.0, chip.1),
        seg(format!(" {}  ", status.resource), Style::default()),
        seg("Cycle: ", label_style()),
        seg(format!("{:.1}ms  ", status.cycle_last), value_style()),
        seg("Uptime: ", label_style()),
        seg(uptime, value_style()),
    ]);
    let web = state
        .data
        .settings
        .as_ref()
        .map(|s| format!("http://{}", s.web_listen))
        .unwrap_or_else(|| "--".to_string());
    let line2 = PromptLine::from_segments(vec![
        seg("I/O: ", label_style()),
        seg(
            status
                .drivers
                .first()
                .map(|d| d.name.as_str())
                .unwrap_or("unknown"),
            value_style(),
        ),
        seg("  Web: ", label_style()),
        seg(web, value_style()),
    ]);
    vec![line, line2]
}

fn info_lines(state: &UiState) -> Vec<PromptLine> {
    let uptime = state
        .data
        .status
        .as_ref()
        .map(|s| format_uptime(s.uptime_ms))
        .unwrap_or_else(|| "--:--:--".to_string());
    vec![
        PromptLine::from_segments(vec![
            seg("Version: ", label_style()),
            seg(env!("CARGO_PKG_VERSION"), value_style()),
        ]),
        PromptLine::from_segments(vec![
            seg("Uptime: ", label_style()),
            seg(uptime, value_style()),
        ]),
    ]
}

struct SettingsMenuEntry {
    key: Option<SettingKey>,
    label: &'static str,
    value: String,
}

#[derive(Clone, Copy, Debug)]
struct MenuEntry {
    label: &'static str,
    command: &'static str,
    needs_input: bool,
}

fn settings_menu_entries(state: &UiState) -> Vec<SettingsMenuEntry> {
    let settings = state.data.settings.clone().unwrap_or_default();
    let name = state
        .data
        .status
        .as_ref()
        .map(|s| s.resource.as_str())
        .unwrap_or("plc")
        .to_string();
    vec![
        SettingsMenuEntry {
            key: Some(SettingKey::PlcName),
            label: "PLC name",
            value: name,
        },
        SettingsMenuEntry {
            key: Some(SettingKey::CycleInterval),
            label: "Cycle interval",
            value: read_cycle_interval_ms(state)
                .map(|ms| format!("{ms} ms"))
                .unwrap_or_else(|| "--".to_string()),
        },
        SettingsMenuEntry {
            key: Some(SettingKey::LogLevel),
            label: "Log level",
            value: settings.log_level,
        },
        SettingsMenuEntry {
            key: Some(SettingKey::ControlMode),
            label: "Control mode",
            value: settings.control_mode,
        },
        SettingsMenuEntry {
            key: Some(SettingKey::WebListen),
            label: "Web listen",
            value: settings.web_listen,
        },
        SettingsMenuEntry {
            key: Some(SettingKey::WebAuth),
            label: "Web auth",
            value: settings.web_auth,
        },
        SettingsMenuEntry {
            key: Some(SettingKey::DiscoveryEnabled),
            label: "Discovery",
            value: if settings.discovery_enabled {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
        },
        SettingsMenuEntry {
            key: Some(SettingKey::MeshEnabled),
            label: "PLC Linking",
            value: if settings.mesh_enabled {
                "enabled".to_string()
            } else {
                "disabled".to_string()
            },
        },
        SettingsMenuEntry {
            key: None,
            label: "Back",
            value: String::new(),
        },
    ]
}

fn read_cycle_interval_ms(state: &UiState) -> Option<u64> {
    let root = state.bundle_root.as_ref()?;
    let path = root.join("runtime.toml");
    let text = fs::read_to_string(path).ok()?;
    let doc: toml::Value = text.parse().ok()?;
    doc.get("resource")?
        .get("cycle_interval_ms")?
        .as_integer()
        .map(|value| value as u64)
}

fn normalize_setting_input(key: SettingKey, value: &str) -> String {
    match key {
        SettingKey::CycleInterval => value
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string(),
        SettingKey::DiscoveryEnabled | SettingKey::MeshEnabled => {
            if value.eq_ignore_ascii_case("enabled") {
                "true".to_string()
            } else if value.eq_ignore_ascii_case("disabled") {
                "false".to_string()
            } else {
                value.to_string()
            }
        }
        _ => value.to_string(),
    }
}

fn settings_menu_lines(state: &UiState, selected: usize) -> Vec<PromptLine> {
    let entries = settings_menu_entries(state);
    let mut lines = Vec::new();
    lines.push(PromptLine::plain("Settings", header_style()));
    for (idx, entry) in entries.iter().enumerate() {
        let highlight = idx == selected;
        if highlight {
            let line = if entry.key.is_some() {
                format!("{:<16} {}", entry.label, entry.value)
            } else {
                entry.label.to_string()
            };
            lines.push(PromptLine::plain(
                line,
                Style::default()
                    .bg(COLOR_TEAL)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
            continue;
        }
        if entry.key.is_some() {
            lines.push(PromptLine::from_segments(vec![
                seg(format!("{:<16} ", entry.label), label_style()),
                seg(entry.value.clone(), value_style()),
            ]));
        } else {
            lines.push(PromptLine::from_segments(vec![seg(
                entry.label,
                label_style(),
            )]));
        }
    }
    lines.push(PromptLine::plain(
        "Use ↑/↓ and Enter. Esc to go back.",
        Style::default().fg(COLOR_INFO),
    ));
    lines
}

fn move_settings_selection(state: &mut UiState, delta: i32) {
    let entries = settings_menu_entries(state);
    let len = entries.len();
    if len == 0 {
        return;
    }
    let mut next = state.settings_index as i32 + delta;
    if next < 0 {
        next = len as i32 - 1;
    } else if next >= len as i32 {
        next = 0;
    }
    state.settings_index = next as usize;
    state
        .prompt
        .set_output(settings_menu_lines(state, state.settings_index));
}

fn menu_title(kind: MenuKind) -> &'static str {
    match kind {
        MenuKind::Io => "I/O Menu",
        MenuKind::Control => "Control Menu",
        MenuKind::Access => "Access Menu",
        MenuKind::Linking => "PLC Linking Menu",
        MenuKind::Log => "Log Menu",
        MenuKind::Restart => "Restart Required",
    }
}

fn menu_entries(kind: MenuKind) -> Vec<MenuEntry> {
    match kind {
        MenuKind::Io => vec![
            MenuEntry {
                label: "Read value",
                command: "/io read",
                needs_input: true,
            },
            MenuEntry {
                label: "Set value",
                command: "/io set",
                needs_input: true,
            },
            MenuEntry {
                label: "Force value",
                command: "/io force",
                needs_input: true,
            },
            MenuEntry {
                label: "Release force",
                command: "/io unforce",
                needs_input: true,
            },
            MenuEntry {
                label: "List all I/O",
                command: "/io list",
                needs_input: false,
            },
            MenuEntry {
                label: "List forced",
                command: "/io forced",
                needs_input: false,
            },
            MenuEntry {
                label: "Back",
                command: "",
                needs_input: false,
            },
        ],
        MenuKind::Control => vec![
            MenuEntry {
                label: "Pause",
                command: "/control pause",
                needs_input: false,
            },
            MenuEntry {
                label: "Resume",
                command: "/control resume",
                needs_input: false,
            },
            MenuEntry {
                label: "Step into",
                command: "/control step",
                needs_input: false,
            },
            MenuEntry {
                label: "Step over",
                command: "/control step-over",
                needs_input: false,
            },
            MenuEntry {
                label: "Step out",
                command: "/control step-out",
                needs_input: false,
            },
            MenuEntry {
                label: "Restart (warm/cold)",
                command: "/control restart",
                needs_input: true,
            },
            MenuEntry {
                label: "Shutdown",
                command: "/control shutdown",
                needs_input: false,
            },
            MenuEntry {
                label: "Set breakpoint",
                command: "/control break",
                needs_input: true,
            },
            MenuEntry {
                label: "List breakpoints",
                command: "/control breaks",
                needs_input: false,
            },
            MenuEntry {
                label: "Delete breakpoint",
                command: "/control delete",
                needs_input: true,
            },
            MenuEntry {
                label: "Back",
                command: "",
                needs_input: false,
            },
        ],
        MenuKind::Access => vec![
            MenuEntry {
                label: "Generate access code",
                command: "/access start",
                needs_input: false,
            },
            MenuEntry {
                label: "Claim access code",
                command: "/access claim",
                needs_input: true,
            },
            MenuEntry {
                label: "List tokens",
                command: "/access list",
                needs_input: false,
            },
            MenuEntry {
                label: "Revoke token",
                command: "/access revoke",
                needs_input: true,
            },
            MenuEntry {
                label: "Back",
                command: "",
                needs_input: false,
            },
        ],
        MenuKind::Linking => vec![
            MenuEntry {
                label: "Enable linking",
                command: "/linking enable",
                needs_input: false,
            },
            MenuEntry {
                label: "Disable linking",
                command: "/linking disable",
                needs_input: false,
            },
            MenuEntry {
                label: "Publish variable",
                command: "/linking publish",
                needs_input: true,
            },
            MenuEntry {
                label: "Subscribe variable",
                command: "/linking subscribe",
                needs_input: true,
            },
            MenuEntry {
                label: "Back",
                command: "",
                needs_input: false,
            },
        ],
        MenuKind::Log => vec![
            MenuEntry {
                label: "Show level",
                command: "/log",
                needs_input: false,
            },
            MenuEntry {
                label: "Set info",
                command: "/log info",
                needs_input: false,
            },
            MenuEntry {
                label: "Set warn",
                command: "/log warn",
                needs_input: false,
            },
            MenuEntry {
                label: "Set debug",
                command: "/log debug",
                needs_input: false,
            },
            MenuEntry {
                label: "Tail logs",
                command: "/log tail",
                needs_input: true,
            },
            MenuEntry {
                label: "Back",
                command: "",
                needs_input: false,
            },
        ],
        MenuKind::Restart => vec![
            MenuEntry {
                label: "Restart now (warm)",
                command: "/control restart warm",
                needs_input: false,
            },
            MenuEntry {
                label: "Restart now (cold) — resets variables",
                command: "/control restart cold",
                needs_input: false,
            },
            MenuEntry {
                label: "Restart later",
                command: "",
                needs_input: false,
            },
        ],
    }
}

fn menu_lines(kind: MenuKind, selected: usize) -> Vec<PromptLine> {
    let entries = menu_entries(kind);
    let mut lines = Vec::new();
    lines.push(PromptLine::plain(menu_title(kind), header_style()));
    if kind == MenuKind::Restart {
        lines.push(PromptLine::plain(
            "Saved. Restart required.",
            Style::default().fg(COLOR_AMBER),
        ));
    }
    if entries.is_empty() {
        lines.push(PromptLine::plain(
            "No options.",
            Style::default().fg(COLOR_INFO),
        ));
        return lines;
    }
    for (idx, entry) in entries.iter().enumerate() {
        if selected == idx {
            let style = Style::default()
                .bg(COLOR_TEAL)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD);
            let mut text = entry.label.to_string();
            if !entry.command.is_empty() {
                text.push(' ');
                text.push_str(entry.command);
            }
            lines.push(PromptLine::plain(text, style));
        } else {
            let mut segs = vec![seg(entry.label, value_style())];
            if !entry.command.is_empty() {
                segs.push(seg(" ", value_style()));
                segs.push(seg(entry.command, Style::default().fg(COLOR_CYAN)));
            }
            lines.push(PromptLine::from_segments(segs));
        }
    }
    lines.push(PromptLine::plain(
        "Use ↑/↓ and Enter. Esc to go back.",
        Style::default().fg(COLOR_INFO),
    ));
    lines
}

fn move_menu_selection(state: &mut UiState, kind: MenuKind, delta: i32) {
    let entries = menu_entries(kind);
    let len = entries.len();
    if len == 0 {
        return;
    }
    let mut next = state.menu_index as i32 + delta;
    if next < 0 {
        next = len as i32 - 1;
    } else if next >= len as i32 {
        next = 0;
    }
    state.menu_index = next as usize;
    state.prompt.set_output(menu_lines(kind, state.menu_index));
}

fn open_menu(kind: MenuKind, state: &mut UiState) {
    state.prompt.mode = PromptMode::Menu(kind);
    state.menu_index = 0;
    state.prompt.set_output(menu_lines(kind, state.menu_index));
    state.prompt.activate_with("");
}

fn io_action_label(action: IoActionKind) -> &'static str {
    match action {
        IoActionKind::Read => "Read I/O",
        IoActionKind::Set => "Set I/O value",
        IoActionKind::Force => "Force I/O value",
        IoActionKind::Unforce => "Release I/O force",
    }
}

fn io_entries_for_action(state: &UiState, action: IoActionKind) -> Vec<usize> {
    let mut indices = Vec::new();
    for (idx, entry) in state.data.io.iter().enumerate() {
        if matches!(
            action,
            IoActionKind::Set | IoActionKind::Force | IoActionKind::Unforce
        ) && !entry.direction.eq_ignore_ascii_case("OUT")
        {
            continue;
        }
        indices.push(idx);
    }
    indices
}

fn io_select_lines(state: &UiState, action: IoActionKind, selected: usize) -> Vec<PromptLine> {
    let indices = io_entries_for_action(state, action);
    let mut lines = Vec::new();
    lines.push(PromptLine::plain(io_action_label(action), header_style()));
    lines.push(PromptLine::from_segments(vec![
        seg("DIR ", header_style()),
        seg("ADDR    ", header_style()),
        seg("NAME       ", header_style()),
        seg("VALUE", header_style()),
    ]));
    if indices.is_empty() {
        lines.push(PromptLine::plain(
            "No matching I/O.",
            Style::default().fg(COLOR_INFO),
        ));
        return lines;
    }
    for (row, idx) in indices.iter().enumerate() {
        let entry = &state.data.io[*idx];
        let forced = if state.forced_io.contains(&entry.address) {
            " *"
        } else {
            ""
        };
        let line_text = format!(
            "{:<3} {:<7} {:<10} {}{forced}",
            entry.direction, entry.address, entry.name, entry.value
        );
        if row == selected {
            lines.push(PromptLine::plain(
                line_text,
                Style::default()
                    .bg(COLOR_TEAL)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            lines.push(PromptLine::from_segments(vec![
                seg(format!("{:<3} ", entry.direction), label_style()),
                seg(format!("{:<7} ", entry.address), value_style()),
                seg(format!("{:<10} ", entry.name), value_style()),
                seg(entry.value.clone(), value_style()),
                seg(forced, Style::default().fg(COLOR_AMBER)),
            ]));
        }
    }
    lines.push(PromptLine::plain(
        "Use ↑/↓ and Enter. Esc to back.",
        Style::default().fg(COLOR_INFO),
    ));
    lines
}

fn open_io_select(action: IoActionKind, state: &mut UiState) {
    state.prompt.mode = PromptMode::IoSelect(action);
    state.io_index = 0;
    state
        .prompt
        .set_output(io_select_lines(state, action, state.io_index));
    state.prompt.activate_with("");
}

fn move_io_selection(state: &mut UiState, action: IoActionKind, delta: i32) {
    let indices = io_entries_for_action(state, action);
    let len = indices.len();
    if len == 0 {
        return;
    }
    let mut next = state.io_index as i32 + delta;
    if next < 0 {
        next = len as i32 - 1;
    } else if next >= len as i32 {
        next = 0;
    }
    state.io_index = next as usize;
    state
        .prompt
        .set_output(io_select_lines(state, action, state.io_index));
}

fn io_value_lines(state: &UiState, selected: usize) -> Vec<PromptLine> {
    let mut lines = Vec::new();
    let address = state.io_pending_address.as_deref().unwrap_or("<io>");
    let action = state
        .io_pending_action
        .map(io_action_label)
        .unwrap_or("I/O");
    lines.push(PromptLine::plain(
        format!("{action} — {address}"),
        header_style(),
    ));
    lines.push(PromptLine::plain(
        "Select value:",
        Style::default().fg(COLOR_INFO),
    ));
    let options = ["TRUE", "FALSE", "Back"];
    for (idx, option) in options.iter().enumerate() {
        if idx == selected {
            lines.push(PromptLine::plain(
                (*option).to_string(),
                Style::default()
                    .bg(COLOR_TEAL)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            lines.push(PromptLine::from_segments(vec![seg(*option, value_style())]));
        }
    }
    lines
}

fn open_io_value_select(action: IoActionKind, address: String, state: &mut UiState) {
    state.io_pending_action = Some(action);
    state.io_pending_address = Some(address);
    state.io_value_index = 0;
    state.prompt.mode = PromptMode::IoValueSelect;
    state
        .prompt
        .set_output(io_value_lines(state, state.io_value_index));
    state.prompt.activate_with("");
}

fn move_io_value_selection(state: &mut UiState, delta: i32) {
    let options_len: i32 = 3;
    let mut next = state.io_value_index as i32 + delta;
    if next < 0 {
        next = options_len - 1;
    } else if next >= options_len {
        next = 0;
    }
    state.io_value_index = next as usize;
    state
        .prompt
        .set_output(io_value_lines(state, state.io_value_index));
}

fn handle_io_value_select(
    input: &str,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    let action = match state.io_pending_action {
        Some(action) => action,
        None => {
            state.prompt.mode = PromptMode::Normal;
            return Ok(false);
        }
    };
    let address = match state.io_pending_address.clone() {
        Some(addr) => addr,
        None => {
            state.prompt.mode = PromptMode::Normal;
            return Ok(false);
        }
    };
    let choice = input.trim();
    let selected = if choice.is_empty() {
        Some(state.io_value_index)
    } else if let Ok(num) = choice.parse::<usize>() {
        if num == 0 {
            Some(2)
        } else {
            num.checked_sub(1)
        }
    } else {
        None
    };
    let Some(selected) = selected else {
        state.prompt.set_output(vec![PromptLine::plain(
            "Invalid choice.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    };
    match selected {
        0 | 1 => {
            let value = if selected == 0 { "true" } else { "false" };
            state.prompt.mode = PromptMode::Normal;
            state.prompt.clear_output();
            match action {
                IoActionKind::Set => {
                    let response = client.request(json!({
                        "id": 1,
                        "type": "io.write",
                        "params": { "address": address, "value": value }
                    }));
                    set_simple_response(state, response, "I/O set queued.");
                }
                IoActionKind::Force => {
                    let response = client.request(json!({
                        "id": 1,
                        "type": "io.force",
                        "params": { "address": address, "value": value }
                    }));
                    state.forced_io.insert(address);
                    set_simple_response(state, response, "I/O forced.");
                }
                _ => {}
            }
        }
        _ => {
            open_io_select(action, state);
        }
    }
    Ok(false)
}

fn handle_io_select(
    input: &str,
    action: IoActionKind,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    let indices = io_entries_for_action(state, action);
    if indices.is_empty() {
        state.prompt.mode = PromptMode::Normal;
        return Ok(false);
    }
    let choice = input.trim();
    let selected = if choice.is_empty() {
        Some(state.io_index)
    } else if let Ok(num) = choice.parse::<usize>() {
        if num == 0 {
            None
        } else {
            num.checked_sub(1)
        }
    } else {
        None
    };
    let Some(selected) = selected else {
        state.prompt.set_output(vec![PromptLine::plain(
            "Invalid choice.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    };
    if selected >= indices.len() {
        state.prompt.set_output(vec![PromptLine::plain(
            "Invalid choice.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    }
    let entry = &state.data.io[indices[selected]];
    let address = entry.address.clone();
    state.prompt.mode = PromptMode::Normal;
    state.prompt.clear_output();
    match action {
        IoActionKind::Read => {
            handle_io_command(vec!["read", &address], client, state)?;
        }
        IoActionKind::Set => {
            if is_bool_value(&entry.value) {
                open_io_value_select(action, address, state);
            } else {
                let cmd = format!("/io set {} ", address);
                state.prompt.activate_with(&cmd);
                state.prompt.set_output(vec![PromptLine::plain(
                    "Enter value:",
                    Style::default().fg(COLOR_INFO),
                )]);
            }
        }
        IoActionKind::Force => {
            if is_bool_value(&entry.value) {
                open_io_value_select(action, address, state);
            } else {
                let cmd = format!("/io force {} ", address);
                state.prompt.activate_with(&cmd);
                state.prompt.set_output(vec![PromptLine::plain(
                    "Enter value:",
                    Style::default().fg(COLOR_INFO),
                )]);
            }
        }
        IoActionKind::Unforce => {
            handle_io_command(vec!["unforce", &address], client, state)?;
        }
    }
    Ok(false)
}

fn handle_menu_select(
    input: &str,
    kind: MenuKind,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<bool> {
    let entries = menu_entries(kind);
    if entries.is_empty() {
        state.prompt.mode = PromptMode::Normal;
        return Ok(false);
    }
    let choice = input.trim();
    let selected = if choice.is_empty() {
        Some(state.menu_index)
    } else if let Ok(num) = choice.parse::<usize>() {
        if num == 0 {
            Some(entries.len().saturating_sub(1))
        } else {
            num.checked_sub(1)
        }
    } else {
        None
    };
    let Some(selected) = selected else {
        state.prompt.set_output(vec![PromptLine::plain(
            "Invalid choice.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    };
    if selected >= entries.len() {
        state.prompt.set_output(vec![PromptLine::plain(
            "Invalid choice.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(false);
    }
    let entry = entries[selected];
    if entry.command.is_empty() {
        state.prompt.clear_output();
        state.prompt.mode = PromptMode::Normal;
        return Ok(false);
    }
    state.prompt.mode = PromptMode::Normal;
    state.prompt.clear_output();
    if kind == MenuKind::Io {
        match entry.command {
            "/io read" => {
                open_io_select(IoActionKind::Read, state);
                return Ok(false);
            }
            "/io set" => {
                open_io_select(IoActionKind::Set, state);
                return Ok(false);
            }
            "/io force" => {
                open_io_select(IoActionKind::Force, state);
                return Ok(false);
            }
            "/io unforce" => {
                open_io_select(IoActionKind::Unforce, state);
                return Ok(false);
            }
            _ => {}
        }
    }
    if entry.needs_input {
        let mut cmd = entry.command.to_string();
        if !cmd.ends_with(' ') {
            cmd.push(' ');
        }
        state.prompt.activate_with(&cmd);
        return Ok(false);
    }
    execute_command(entry.command, client, state)
}

fn handle_io_command(
    args: Vec<&str>,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<()> {
    if args.is_empty() {
        open_menu(MenuKind::Io, state);
        return Ok(());
    }
    match args[0] {
        "list" => {
            let mut lines = Vec::new();
            for entry in state.data.io.iter() {
                lines.push(PromptLine::plain(
                    format!(
                        "{} {}{} {}",
                        entry.direction,
                        if entry.name.is_empty() {
                            ""
                        } else {
                            entry.name.as_str()
                        },
                        entry.address,
                        entry.value
                    ),
                    Style::default().fg(COLOR_INFO),
                ));
            }
            state.prompt.set_output(lines);
        }
        "read" => {
            if args.get(1).is_none() {
                open_io_select(IoActionKind::Read, state);
                return Ok(());
            }
            if let Some(addr) = args.get(1) {
                if let Some(entry) = state.data.io.iter().find(|e| &e.address == addr) {
                    state.prompt.set_output(vec![PromptLine::plain(
                        format!("{} = {}", entry.address, entry.value),
                        Style::default().fg(COLOR_INFO),
                    )]);
                } else {
                    state.prompt.set_output(vec![PromptLine::plain(
                        "Address not found.",
                        Style::default().fg(COLOR_RED),
                    )]);
                }
            }
        }
        "set" => {
            if args.len() < 3 {
                open_io_select(IoActionKind::Set, state);
                return Ok(());
            }
            let response = client.request(json!({
                "id": 1,
                "type": "io.write",
                "params": { "address": args[1], "value": args[2] }
            }));
            set_simple_response(state, response, "I/O set queued.");
        }
        "force" => {
            if args.len() < 3 {
                open_io_select(IoActionKind::Force, state);
                return Ok(());
            }
            let response = client.request(json!({
                "id": 1,
                "type": "io.force",
                "params": { "address": args[1], "value": args[2] }
            }));
            state.forced_io.insert(args[1].to_string());
            set_simple_response(state, response, "I/O forced.");
        }
        "unforce" => {
            if args.len() < 2 {
                open_io_select(IoActionKind::Unforce, state);
                return Ok(());
            }
            if args[1] == "all" {
                for addr in state.forced_io.clone() {
                    let _ = client.request(json!({
                        "id": 1,
                        "type": "io.unforce",
                        "params": { "address": addr }
                    }));
                }
                state.forced_io.clear();
                state.prompt.set_output(vec![PromptLine::plain(
                    "All forced I/O released.",
                    Style::default().fg(COLOR_INFO),
                )]);
            } else {
                let response = client.request(json!({
                    "id": 1,
                    "type": "io.unforce",
                    "params": { "address": args[1] }
                }));
                state.forced_io.remove(args[1]);
                set_simple_response(state, response, "I/O released.");
            }
        }
        "forced" => {
            if state.forced_io.is_empty() {
                state.prompt.set_output(vec![PromptLine::plain(
                    "No forced I/O.",
                    Style::default().fg(COLOR_INFO),
                )]);
            } else {
                let lines = state
                    .forced_io
                    .iter()
                    .map(|addr| PromptLine::plain(addr.clone(), Style::default().fg(COLOR_INFO)))
                    .collect::<Vec<_>>();
                state.prompt.set_output(lines);
            }
        }
        _ => {
            state.prompt.set_output(vec![PromptLine::plain(
                "Unknown /io command.",
                Style::default().fg(COLOR_RED),
            )]);
        }
    }
    Ok(())
}

fn handle_control_command(
    args: Vec<&str>,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<()> {
    if args.is_empty() {
        open_menu(MenuKind::Control, state);
        return Ok(());
    }
    match args[0] {
        "pause" => {
            let response = client.request(json!({"id": 1, "type": "pause"}));
            set_simple_response(state, response, "Paused.");
        }
        "resume" => {
            let response = client.request(json!({"id": 1, "type": "resume"}));
            set_simple_response(state, response, "Resumed.");
        }
        "step" => {
            let response = client.request(json!({"id": 1, "type": "step_in"}));
            set_simple_response(state, response, "Step.");
        }
        "step-over" => {
            let response = client.request(json!({"id": 1, "type": "step_over"}));
            set_simple_response(state, response, "Step over.");
        }
        "step-out" => {
            let response = client.request(json!({"id": 1, "type": "step_out"}));
            set_simple_response(state, response, "Step out.");
        }
        "restart" => {
            if args.len() < 2 {
                open_menu(MenuKind::Restart, state);
                return Ok(());
            }
            let mode = args.get(1).copied().unwrap_or("warm");
            let response =
                client.request(json!({"id": 1, "type": "restart", "params": { "mode": mode }}));
            set_simple_response(state, response, "Restarting...");
        }
        "shutdown" => {
            state.prompt.mode = PromptMode::ConfirmAction(ConfirmAction::Shutdown);
            state.prompt.set_output(vec![PromptLine::plain(
                "This will stop the PLC. Are you sure? [y/N]",
                Style::default().fg(COLOR_AMBER),
            )]);
            state.prompt.activate_with("");
        }
        "break" => {
            if let Some(loc) = args.get(1) {
                if let Some((file, line)) = loc.split_once(':') {
                    let line_num = line.parse::<u32>().unwrap_or(1);
                    let response = client.request(json!({
                        "id": 1,
                        "type": "breakpoints.set",
                        "params": { "source": file, "lines": [line_num] }
                    }));
                    set_simple_response(state, response, "Breakpoint set.");
                }
            }
        }
        "breaks" => {
            let response = client.request(json!({"id": 1, "type": "breakpoints.list"}));
            match response {
                Ok(value) => {
                    if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
                        state.prompt.set_output(vec![PromptLine::plain(
                            err.to_string(),
                            Style::default().fg(COLOR_RED),
                        )]);
                    } else if let Some(list) = value
                        .get("result")
                        .and_then(|r| r.get("breakpoints"))
                        .and_then(|v| v.as_array())
                    {
                        if list.is_empty() {
                            state.prompt.set_output(vec![PromptLine::plain(
                                "No breakpoints.",
                                Style::default().fg(COLOR_INFO),
                            )]);
                        } else {
                            let mut lines = Vec::new();
                            for bp in list {
                                let file_id =
                                    bp.get("file_id").and_then(|v| v.as_u64()).unwrap_or(0);
                                let start = bp.get("start").and_then(|v| v.as_u64()).unwrap_or(0);
                                lines.push(PromptLine::plain(
                                    format!("file {file_id} @ {start}"),
                                    Style::default().fg(COLOR_INFO),
                                ));
                            }
                            state.prompt.set_output(lines);
                        }
                    }
                }
                Err(err) => {
                    state.prompt.set_output(vec![PromptLine::plain(
                        format!("Error: {err}"),
                        Style::default().fg(COLOR_RED),
                    )]);
                }
            }
        }
        "delete" => {
            if let Some(target) = args.get(1) {
                if *target == "all" {
                    let response =
                        client.request(json!({"id": 1, "type": "breakpoints.clear_all"}));
                    set_simple_response(state, response, "Breakpoints cleared.");
                } else if let Ok(id) = target.parse::<u32>() {
                    let response = client.request(json!({
                        "id": 1,
                        "type": "breakpoints.clear_id",
                        "params": { "file_id": id }
                    }));
                    set_simple_response(state, response, "Breakpoint cleared.");
                }
            }
        }
        _ => {
            state.prompt.set_output(vec![PromptLine::plain(
                "Unknown /control command.",
                Style::default().fg(COLOR_RED),
            )]);
        }
    }
    Ok(())
}

fn handle_access_command(
    args: Vec<&str>,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<()> {
    if args.is_empty() {
        open_menu(MenuKind::Access, state);
        return Ok(());
    }
    match args[0] {
        "start" => {
            let response = client.request(json!({"id": 1, "type": "pair.start"}));
            if let Ok(value) = response {
                if let Some(code) = value
                    .get("result")
                    .and_then(|r| r.get("code"))
                    .and_then(|v| v.as_str())
                {
                    state.prompt.set_output(vec![PromptLine::plain(
                        format!("Access code: {code} (valid 5 min)"),
                        Style::default().fg(COLOR_GREEN),
                    )]);
                } else {
                    set_simple_response(state, Ok(value), "Access code generated.");
                }
            }
        }
        "claim" => {
            if let Some(code) = args.get(1) {
                let response = client.request(json!({
                    "id": 1,
                    "type": "pair.claim",
                    "params": { "code": code }
                }));
                if let Ok(value) = response {
                    if let Some(token) = value
                        .get("result")
                        .and_then(|r| r.get("token"))
                        .and_then(|v| v.as_str())
                    {
                        state.prompt.set_output(vec![PromptLine::plain(
                            format!("Token: {token}"),
                            Style::default().fg(COLOR_GREEN),
                        )]);
                    } else {
                        set_simple_response(state, Ok(value), "Claimed.");
                    }
                }
            }
        }
        "list" => {
            let response = client.request(json!({"id": 1, "type": "pair.list"}));
            set_simple_response(state, response, "Tokens:");
        }
        "revoke" => {
            if let Some(id) = args.get(1) {
                let response = client.request(json!({
                    "id": 1,
                    "type": "pair.revoke",
                    "params": { "id": id }
                }));
                set_simple_response(state, response, "Revoked.");
            } else {
                state.prompt.set_output(vec![PromptLine::plain(
                    "Usage: /access revoke <id|all>",
                    Style::default().fg(COLOR_INFO),
                )]);
            }
        }
        _ => {
            state.prompt.set_output(vec![PromptLine::plain(
                "Unknown /access command.",
                Style::default().fg(COLOR_RED),
            )]);
        }
    }
    Ok(())
}

fn handle_linking_command(
    args: Vec<&str>,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<()> {
    if args.is_empty() {
        open_menu(MenuKind::Linking, state);
        return Ok(());
    }
    let settings = state.data.settings.clone().unwrap_or_default();
    match args[0] {
        "enable" | "disable" => {
            let enabled = args[0] == "enable";
            let response = config_set(client, json!({ "mesh.enabled": enabled }));
            set_config_response(state, response, "Saved.");
        }
        "publish" => {
            if let Some(var) = args.get(1) {
                let mut publish = settings.mesh_publish.clone();
                if !publish.iter().any(|v| v == var) {
                    publish.push(var.to_string());
                }
                let response = config_set(client, json!({ "mesh.publish": publish }));
                set_config_response(state, response, "Saved.");
            }
        }
        "subscribe" => {
            if args.len() >= 3 {
                let mut subscribe = settings
                    .mesh_subscribe
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>();
                subscribe.insert(args[1].to_string(), args[2].to_string());
                let map = subscribe
                    .iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect::<serde_json::Map<_, _>>();
                let response = config_set(client, json!({ "mesh.subscribe": map }));
                set_config_response(state, response, "Saved.");
            }
        }
        _ => {
            state.prompt.set_output(vec![PromptLine::plain(
                "Unknown /linking command.",
                Style::default().fg(COLOR_RED),
            )]);
        }
    }
    Ok(())
}

fn handle_log_command(
    args: Vec<&str>,
    client: &mut ControlClient,
    state: &mut UiState,
) -> anyhow::Result<()> {
    if args.is_empty() {
        open_menu(MenuKind::Log, state);
        return Ok(());
    }
    if args[0] == "tail" {
        let limit = args
            .get(1)
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10);
        let response =
            client.request(json!({"id": 1, "type": "events.tail", "params": { "limit": limit }}));
        match response {
            Ok(value) => {
                if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
                    state.prompt.set_output(vec![PromptLine::plain(
                        err.to_string(),
                        Style::default().fg(COLOR_RED),
                    )]);
                } else {
                    let events = parse_events(&value);
                    if events.is_empty() {
                        state.prompt.set_output(vec![PromptLine::plain(
                            "No events.",
                            Style::default().fg(COLOR_INFO),
                        )]);
                    } else {
                        let lines = events
                            .into_iter()
                            .map(|event| {
                                PromptLine::plain(event.label, Style::default().fg(COLOR_INFO))
                            })
                            .collect();
                        state.prompt.set_output(lines);
                    }
                }
            }
            Err(err) => {
                state.prompt.set_output(vec![PromptLine::plain(
                    format!("Error: {err}"),
                    Style::default().fg(COLOR_RED),
                )]);
            }
        }
        return Ok(());
    }
    let response = config_set(client, json!({ "log.level": args[0] }));
    set_config_response(state, response, "Saved.");
    Ok(())
}

fn handle_layout_command(args: Vec<&str>, state: &mut UiState) -> anyhow::Result<()> {
    if args.is_empty() {
        let names = state
            .layout
            .iter()
            .map(|p| format!("{:?}", p).to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        state.prompt.set_output(vec![PromptLine::plain(
            format!("Current: {names}"),
            Style::default().fg(COLOR_INFO),
        )]);
        return Ok(());
    }
    let mut panels = Vec::new();
    for arg in args.iter().take(4) {
        if let Some(panel) = PanelKind::parse(arg) {
            if !panels.contains(&panel) {
                panels.push(panel);
            }
        }
    }
    if !panels.is_empty() {
        while panels.len() < 4 {
            panels.push(PanelKind::Status);
        }
        state.layout = panels;
        state.panel_page = 0;
        state.prompt.set_output(vec![PromptLine::plain(
            "Layout updated.",
            Style::default().fg(COLOR_GREEN),
        )]);
    }
    Ok(())
}

fn handle_focus_command(args: Vec<&str>, state: &mut UiState) -> anyhow::Result<()> {
    if let Some(name) = args.first() {
        if let Some(panel) = PanelKind::parse(name) {
            state.focus = Some(panel);
            state.prompt.set_output(vec![PromptLine::plain(
                format!("Focused {name}."),
                Style::default().fg(COLOR_INFO),
            )]);
        }
    }
    Ok(())
}

fn handle_build_command(state: &mut UiState) -> anyhow::Result<()> {
    let Some(root) = state.bundle_root.as_ref() else {
        state.prompt.set_output(vec![PromptLine::plain(
            "Project path required.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(());
    };
    match build_program_stbc(root, None) {
        Ok(report) => {
            state.prompt.set_output(vec![PromptLine::plain(
                format!("Built program.stbc ({} sources).", report.sources.len()),
                Style::default().fg(COLOR_GREEN),
            )]);
        }
        Err(err) => {
            state.prompt.set_output(vec![PromptLine::plain(
                format!("Build failed: {err}"),
                Style::default().fg(COLOR_RED),
            )]);
        }
    }
    Ok(())
}

fn handle_reload_command(client: &mut ControlClient, state: &mut UiState) -> anyhow::Result<()> {
    let Some(root) = state.bundle_root.as_ref() else {
        state.prompt.set_output(vec![PromptLine::plain(
            "Project path required.",
            Style::default().fg(COLOR_RED),
        )]);
        return Ok(());
    };
    let path = root.join("program.stbc");
    let bytes = fs::read(&path)?;
    let encoded = BASE64_STANDARD.encode(bytes);
    let response = client.request(json!({
        "id": 1,
        "type": "bytecode.reload",
        "params": { "bytes": encoded }
    }));
    set_simple_response(state, response, "Reloaded.");
    Ok(())
}

struct ConfigSetResult {
    ok: bool,
    restart_required: bool,
    error: Option<String>,
}

fn config_set(client: &mut ControlClient, params: serde_json::Value) -> ConfigSetResult {
    let response = client.request(json!({"id": 1, "type": "config.set", "params": params}));
    if let Ok(value) = response {
        if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
            return ConfigSetResult {
                ok: false,
                restart_required: false,
                error: Some(err.to_string()),
            };
        }
        let restart_required = value
            .get("result")
            .and_then(|r| r.get("restart_required"))
            .and_then(|v| v.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false);
        return ConfigSetResult {
            ok: true,
            restart_required,
            error: None,
        };
    }
    ConfigSetResult {
        ok: false,
        restart_required: false,
        error: Some("request failed".to_string()),
    }
}

fn set_config_response(state: &mut UiState, result: ConfigSetResult, success: &str) {
    if !result.ok {
        state.prompt.set_output(vec![PromptLine::plain(
            result.error.unwrap_or_else(|| "error".into()),
            Style::default().fg(COLOR_RED),
        )]);
        return;
    }
    if result.restart_required {
        open_menu(MenuKind::Restart, state);
    } else {
        state.prompt.set_output(vec![PromptLine::plain(
            success,
            Style::default().fg(COLOR_GREEN),
        )]);
    }
}

fn set_simple_response(
    state: &mut UiState,
    response: anyhow::Result<serde_json::Value>,
    success: &str,
) {
    match response {
        Ok(value) => {
            if let Some(err) = value.get("error").and_then(|v| v.as_str()) {
                state.prompt.set_output(vec![PromptLine::plain(
                    err.to_string(),
                    Style::default().fg(COLOR_RED),
                )]);
            } else {
                state.prompt.set_output(vec![PromptLine::plain(
                    success.to_string(),
                    Style::default().fg(COLOR_GREEN),
                )]);
            }
        }
        Err(err) => {
            state.prompt.set_output(vec![PromptLine::plain(
                format!("Error: {err}"),
                Style::default().fg(COLOR_RED),
            )]);
        }
    }
}

fn update_runtime_toml(root: &Path, key: &str, value: &str) -> anyhow::Result<()> {
    let path = root.join("runtime.toml");
    let text = fs::read_to_string(&path)?;
    let mut doc: toml::Value = text.parse()?;
    set_toml_value(&mut doc, key, value)?;
    let output = toml::to_string_pretty(&doc)?;
    fs::write(&path, output)?;
    Ok(())
}

fn set_toml_value(doc: &mut toml::Value, key: &str, value: &str) -> anyhow::Result<()> {
    let mut parts = key.split('.').peekable();
    let mut current = doc;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            *current
                .as_table_mut()
                .ok_or_else(|| anyhow::anyhow!("invalid toml path"))?
                .entry(part)
                .or_insert(toml::Value::String(value.to_string())) = parse_toml_value(value);
            return Ok(());
        }
        current = current
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("invalid toml path"))?
            .entry(part)
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    }
    Ok(())
}

fn parse_toml_value(value: &str) -> toml::Value {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return toml::Value::Boolean(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return toml::Value::Boolean(false);
    }
    if let Ok(number) = trimmed.parse::<i64>() {
        return toml::Value::Integer(number);
    }
    toml::Value::String(trimmed.to_string())
}

fn parse_bool_value(value: &str) -> Option<bool> {
    let trimmed = value.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "true" | "1" | "yes" | "on" | "enable" | "enabled" => Some(true),
        "false" | "0" | "no" | "off" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

fn is_bool_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("true") || trimmed.eq_ignore_ascii_case("false") {
        return true;
    }
    trimmed.starts_with("Bool(") || trimmed.contains("Bool(")
}

enum ControlStream {
    Tcp(std::net::TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
}

impl Read for ControlStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            ControlStream::Tcp(stream) => stream.read(buf),
            #[cfg(unix)]
            ControlStream::Unix(stream) => stream.read(buf),
        }
    }
}

impl Write for ControlStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            ControlStream::Tcp(stream) => stream.write(buf),
            #[cfg(unix)]
            ControlStream::Unix(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            ControlStream::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            ControlStream::Unix(stream) => stream.flush(),
        }
    }
}

struct ControlClient {
    token: Option<String>,
    reader: io::BufReader<ControlStream>,
}

impl ControlClient {
    fn connect(endpoint: ControlEndpoint, token: Option<String>) -> anyhow::Result<Self> {
        let stream = match &endpoint {
            ControlEndpoint::Tcp(addr) => ControlStream::Tcp(std::net::TcpStream::connect(addr)?),
            #[cfg(unix)]
            ControlEndpoint::Unix(path) => {
                ControlStream::Unix(std::os::unix::net::UnixStream::connect(path)?)
            }
        };
        Ok(Self {
            token,
            reader: io::BufReader::new(stream),
        })
    }

    fn request(&mut self, mut payload: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        if let Some(token) = self.token.as_deref() {
            payload["auth"] = json!(token);
        }
        let line = serde_json::to_string(&payload)?;
        {
            let stream = self.reader.get_mut();
            stream.write_all(line.as_bytes())?;
            stream.write_all(b"\n")?;
            stream.flush()?;
        }
        let mut response = String::new();
        self.reader.read_line(&mut response)?;
        Ok(serde_json::from_str(&response)?)
    }
}
