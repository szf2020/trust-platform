//! Web IDE scope/session model and document editing state.

#![allow(missing_docs)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use glob::Pattern;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use text_size::{TextRange, TextSize};
use trust_hir::db::{FileId, SemanticDatabase, SourceDatabase};
use trust_wasm_analysis::{
    BrowserAnalysisEngine, CompletionItem, CompletionRequest, DiagnosticItem, DocumentInput,
    HoverItem, HoverRequest, Position,
};

const SESSION_TTL_SECS: u64 = 15 * 60;
const MAX_SESSIONS: usize = 16;
const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_FS_AUDIT_EVENTS: usize = 1024;
const ANALYSIS_CACHE_REFRESH_INTERVAL_SECS: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeRole {
    Viewer,
    Editor,
}

impl IdeRole {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "viewer" | "read_only" | "readonly" => Some(Self::Viewer),
            "editor" | "authoring" => Some(Self::Editor),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WebIdeCapabilities {
    pub enabled: bool,
    pub mode: String,
    pub diagnostics_source: String,
    pub deployment_boundaries: Vec<String>,
    pub security_model: Vec<String>,
    pub limits: WebIdeLimits,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebIdeLimits {
    pub session_ttl_secs: u64,
    pub max_sessions: usize,
    pub max_file_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeSession {
    pub token: String,
    pub role: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeFileSnapshot {
    pub path: String,
    pub content: String,
    pub version: u64,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeWriteResult {
    pub path: String,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeFormatResult {
    pub path: String,
    pub content: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeTreeNode {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub children: Vec<IdeTreeNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeFsResult {
    pub path: String,
    pub kind: String,
    pub version: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdePosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeRange {
    pub start: IdePosition,
    pub end: IdePosition,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeLocation {
    pub path: String,
    pub range: IdeRange,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeSearchHit {
    pub path: String,
    pub line: u32,
    pub character: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeSymbolHit {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeRenameResult {
    pub edit_count: usize,
    pub changed_files: Vec<IdeWriteResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeFsAuditRecord {
    pub ts_secs: u64,
    pub session: String,
    pub action: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebIdeHealth {
    pub active_sessions: usize,
    pub editor_sessions: usize,
    pub tracked_documents: usize,
    pub open_document_handles: usize,
    pub fs_mutation_events: usize,
    pub limits: WebIdeLimits,
    pub frontend_telemetry: WebIdeFrontendTelemetry,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeProjectSelection {
    pub active_project: Option<String>,
    pub startup_project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebIdeFrontendTelemetry {
    pub bootstrap_failures: u64,
    pub analysis_timeouts: u64,
    pub worker_restarts: u64,
    pub autosave_failures: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeBrowseEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdeBrowseResult {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<IdeBrowseEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeErrorKind {
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    InvalidInput,
    TooLarge,
    LimitExceeded,
    Internal,
}

#[derive(Debug, Clone)]
pub struct IdeError {
    kind: IdeErrorKind,
    message: String,
    current_version: Option<u64>,
}

impl IdeError {
    fn new(kind: IdeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            current_version: None,
        }
    }

    fn conflict(current_version: u64) -> Self {
        Self {
            kind: IdeErrorKind::Conflict,
            message: format!("edit conflict: current version is {current_version}"),
            current_version: Some(current_version),
        }
    }

    #[must_use]
    pub fn kind(&self) -> IdeErrorKind {
        self.kind
    }

    #[must_use]
    pub fn status_code(&self) -> u16 {
        match self.kind {
            IdeErrorKind::Unauthorized => 401,
            IdeErrorKind::Forbidden => 403,
            IdeErrorKind::NotFound => 404,
            IdeErrorKind::Conflict => 409,
            IdeErrorKind::InvalidInput => 400,
            IdeErrorKind::TooLarge => 413,
            IdeErrorKind::LimitExceeded => 429,
            IdeErrorKind::Internal => 500,
        }
    }

    #[must_use]
    pub fn current_version(&self) -> Option<u64> {
        self.current_version
    }
}

impl fmt::Display for IdeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for IdeError {}

pub struct WebIdeState {
    startup_project_root: Option<PathBuf>,
    active_project_root: Mutex<Option<PathBuf>>,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    limits: WebIdeLimits,
    inner: Mutex<IdeStateInner>,
}

impl fmt::Debug for WebIdeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebIdeState")
            .field("startup_project_root", &self.startup_project_root)
            .field("limits", &self.limits)
            .finish()
    }
}

#[derive(Debug, Default)]
struct IdeStateInner {
    sessions: HashMap<String, IdeSessionEntry>,
    documents: HashMap<String, IdeDocumentEntry>,
    frontend_telemetry_by_session: HashMap<String, WebIdeFrontendTelemetry>,
    analysis_cache: HashMap<String, IdeAnalysisCacheEntry>,
    fs_audit_log: Vec<IdeFsAuditEvent>,
}

#[derive(Debug, Clone)]
struct IdeSessionEntry {
    role: IdeRole,
    expires_at: u64,
    open_paths: BTreeSet<String>,
}

#[derive(Debug, Clone)]
struct IdeDocumentEntry {
    content: String,
    version: u64,
    opened_by: BTreeSet<String>,
}

#[derive(Debug)]
struct IdeAnalysisCacheEntry {
    engine: BrowserAnalysisEngine,
    docs: BTreeMap<String, String>,
    fingerprints: BTreeMap<String, SourceFingerprint>,
    initialized: bool,
    next_refresh_at_secs: u64,
    engine_applied: bool,
}

impl Default for IdeAnalysisCacheEntry {
    fn default() -> Self {
        Self {
            engine: BrowserAnalysisEngine::new(),
            docs: BTreeMap::new(),
            fingerprints: BTreeMap::new(),
            initialized: false,
            next_refresh_at_secs: 0,
            engine_applied: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceFingerprint {
    size_bytes: u64,
    modified_ms: u128,
}

#[derive(Debug, Clone)]
struct IdeFsAuditEvent {
    ts_secs: u64,
    session: String,
    action: String,
    path: String,
}

impl WebIdeState {
    #[must_use]
    pub fn new(project_root: Option<PathBuf>) -> Self {
        Self {
            startup_project_root: project_root.clone(),
            active_project_root: Mutex::new(project_root),
            now: Arc::new(now_secs),
            limits: WebIdeLimits {
                session_ttl_secs: SESSION_TTL_SECS,
                max_sessions: MAX_SESSIONS,
                max_file_bytes: MAX_FILE_BYTES,
            },
            inner: Mutex::new(IdeStateInner::default()),
        }
    }

    #[cfg(test)]
    fn with_clock(project_root: Option<PathBuf>, now: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self {
            startup_project_root: project_root.clone(),
            active_project_root: Mutex::new(project_root),
            now,
            limits: WebIdeLimits {
                session_ttl_secs: SESSION_TTL_SECS,
                max_sessions: MAX_SESSIONS,
                max_file_bytes: MAX_FILE_BYTES,
            },
            inner: Mutex::new(IdeStateInner::default()),
        }
    }

    #[must_use]
    pub fn capabilities(&self, write_enabled: bool) -> WebIdeCapabilities {
        let enabled = self
            .active_project_root
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .is_some();
        WebIdeCapabilities {
            enabled,
            mode: if write_enabled {
                "authoring".to_string()
            } else {
                "read_only".to_string()
            },
            diagnostics_source: "trust-wasm-analysis in-process diagnostics/hover/completion"
                .to_string(),
            deployment_boundaries: vec![
                "Allowed file scope: <project>/**/* (hidden/system paths filtered)".to_string(),
                "Editor session requires engineer/admin web role".to_string(),
            ],
            security_model: vec![
                "Session bootstrap requires web auth (local or X-Trust-Token)".to_string(),
                "Per-session token required for IDE API calls (X-Trust-Ide-Session)".to_string(),
                "Session TTL uses sliding renewal while requests remain active".to_string(),
                "Optimistic concurrency via expected_version prevents blind overwrite".to_string(),
            ],
            limits: self.limits.clone(),
        }
    }

    pub fn project_selection(&self, session_token: &str) -> Result<IdeProjectSelection, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);
        self.current_project_selection()
    }

    pub fn set_active_project(
        &self,
        session_token: &str,
        path: &str,
    ) -> Result<IdeProjectSelection, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);

        let selected = normalize_project_root(path)?;
        let canonical = selected.canonicalize().map_err(|_| {
            IdeError::new(
                IdeErrorKind::NotFound,
                "project root not found or inaccessible",
            )
        })?;
        if !canonical.is_dir() {
            return Err(IdeError::new(
                IdeErrorKind::InvalidInput,
                "project root must be a directory",
            ));
        }

        {
            let mut project_guard = self
                .active_project_root
                .lock()
                .map_err(|_| IdeError::new(IdeErrorKind::Internal, "project root lock poisoned"))?;
            *project_guard = Some(canonical);
        }

        let mut state_guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        state_guard.documents.clear();
        state_guard.analysis_cache.clear();
        for session in state_guard.sessions.values_mut() {
            session.open_paths.clear();
        }
        drop(state_guard);

        self.current_project_selection()
    }

    pub fn active_project_root(&self) -> Option<PathBuf> {
        self.active_project_root
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn current_project_selection(&self) -> Result<IdeProjectSelection, IdeError> {
        let active_project = self
            .active_project_root
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "project root lock poisoned"))?
            .clone()
            .map(pathbuf_to_display);
        let startup_project = self.startup_project_root.clone().map(pathbuf_to_display);
        Ok(IdeProjectSelection {
            active_project,
            startup_project,
        })
    }

    pub fn create_session(&self, role: IdeRole) -> Result<IdeSession, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        prune_expired(&mut guard, now);
        if guard.sessions.len() >= self.limits.max_sessions {
            return Err(IdeError::new(
                IdeErrorKind::LimitExceeded,
                "too many active IDE sessions",
            ));
        }

        let token = generate_token();
        let expires_at = now.saturating_add(self.limits.session_ttl_secs);
        guard.sessions.insert(
            token.clone(),
            IdeSessionEntry {
                role,
                expires_at,
                open_paths: BTreeSet::new(),
            },
        );

        Ok(IdeSession {
            token,
            role: role.as_str().to_string(),
            expires_at,
        })
    }

    pub fn list_sources(&self, session_token: &str) -> Result<Vec<String>, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);

        let root = self.workspace_root()?;
        let mut list = Vec::new();
        collect_workspace_files(&root, &PathBuf::new(), &mut list)?;
        list.sort();
        Ok(list)
    }

    pub fn list_tree(&self, session_token: &str) -> Result<Vec<IdeTreeNode>, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);

        let root = self.workspace_root()?;
        let nodes = collect_workspace_tree(&root, &PathBuf::new())?;
        Ok(nodes)
    }

    pub fn require_editor_session(&self, session_token: &str) -> Result<(), IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_editor_session(&mut guard, session_token, now)?;
        Ok(())
    }

    pub fn create_entry(
        &self,
        session_token: &str,
        path: &str,
        is_directory: bool,
        content: Option<String>,
        write_enabled: bool,
    ) -> Result<IdeFsResult, IdeError> {
        if !write_enabled {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "web IDE authoring is disabled in current runtime mode",
            ));
        }

        let normalized = if is_directory {
            normalize_workspace_path(path, false)?
        } else {
            normalize_workspace_file_path(path)?
        };
        let resolved = self.resolve_workspace_path(&normalized)?;
        if resolved.exists() {
            return Err(IdeError::new(
                IdeErrorKind::Conflict,
                "target path already exists",
            ));
        }

        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_editor_session(&mut guard, session_token, now)?;

        if is_directory {
            std::fs::create_dir_all(&resolved).map_err(|err| {
                IdeError::new(IdeErrorKind::Internal, format!("mkdir failed: {err}"))
            })?;
            guard.analysis_cache.clear();
            self.record_fs_audit_event(
                &mut guard,
                session_token,
                "create_directory",
                normalized.as_str(),
                now,
            );
            return Ok(IdeFsResult {
                path: normalized,
                kind: "directory".to_string(),
                version: None,
            });
        }

        let payload = content.unwrap_or_default();
        if payload.len() > self.limits.max_file_bytes {
            return Err(IdeError::new(
                IdeErrorKind::TooLarge,
                format!(
                    "payload exceeds limit ({} > {} bytes)",
                    payload.len(),
                    self.limits.max_file_bytes
                ),
            ));
        }

        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                IdeError::new(
                    IdeErrorKind::Internal,
                    format!("create parent directory failed: {err}"),
                )
            })?;
        }
        std::fs::write(&resolved, &payload).map_err(|err| {
            IdeError::new(IdeErrorKind::Internal, format!("create file failed: {err}"))
        })?;

        let version = {
            let entry = guard
                .documents
                .entry(normalized.clone())
                .or_insert_with(|| IdeDocumentEntry {
                    content: payload.clone(),
                    version: 1,
                    opened_by: BTreeSet::new(),
                });
            entry.content = payload;
            entry.version = entry.version.max(1);
            entry.opened_by.insert(session_token.to_string());
            entry.version
        };
        self.record_fs_audit_event(
            &mut guard,
            session_token,
            "create_file",
            normalized.as_str(),
            now,
        );
        guard.analysis_cache.clear();

        Ok(IdeFsResult {
            path: normalized,
            kind: "file".to_string(),
            version: Some(version),
        })
    }

    pub fn rename_entry(
        &self,
        session_token: &str,
        path: &str,
        new_path: &str,
        write_enabled: bool,
    ) -> Result<IdeFsResult, IdeError> {
        if !write_enabled {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "web IDE authoring is disabled in current runtime mode",
            ));
        }

        let old_norm = normalize_workspace_path(path, false)?;
        let new_norm = normalize_workspace_path(new_path, false)?;
        let old_resolved = self.resolve_workspace_path(&old_norm)?;
        let old_is_dir = old_resolved.is_dir();
        let new_resolved = self.resolve_workspace_path(&new_norm)?;
        if !old_resolved.exists() {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "source path not found",
            ));
        }
        if new_resolved.exists() {
            return Err(IdeError::new(
                IdeErrorKind::Conflict,
                "target path already exists",
            ));
        }
        if let Some(parent) = new_resolved.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                IdeError::new(
                    IdeErrorKind::Internal,
                    format!("create parent directory failed: {err}"),
                )
            })?;
        }

        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_editor_session(&mut guard, session_token, now)?;

        std::fs::rename(&old_resolved, &new_resolved).map_err(|err| {
            IdeError::new(IdeErrorKind::Internal, format!("rename failed: {err}"))
        })?;

        if old_is_dir {
            let mut remapped_docs = Vec::new();
            for key in guard.documents.keys().cloned().collect::<Vec<_>>() {
                if key == old_norm || key.starts_with(&format!("{old_norm}/")) {
                    let suffix = key.strip_prefix(&old_norm).unwrap_or_default();
                    let mapped = format!("{new_norm}{suffix}");
                    remapped_docs.push((key, mapped));
                }
            }
            for (old_key, mapped) in remapped_docs {
                if let Some(mut entry) = guard.documents.remove(&old_key) {
                    entry.version = entry.version.saturating_add(1);
                    guard.documents.insert(mapped, entry);
                }
            }
            for session in guard.sessions.values_mut() {
                let mut remapped = BTreeSet::new();
                for path in &session.open_paths {
                    if path == &old_norm || path.starts_with(&format!("{old_norm}/")) {
                        let suffix = path.strip_prefix(&old_norm).unwrap_or_default();
                        remapped.insert(format!("{new_norm}{suffix}"));
                    } else {
                        remapped.insert(path.clone());
                    }
                }
                session.open_paths = remapped;
            }
        } else if let Some(mut entry) = guard.documents.remove(&old_norm) {
            entry.version = entry.version.saturating_add(1);
            guard.documents.insert(new_norm.clone(), entry);
            for session in guard.sessions.values_mut() {
                if session.open_paths.remove(&old_norm) {
                    session.open_paths.insert(new_norm.clone());
                }
            }
        }

        guard.analysis_cache.clear();
        self.record_fs_audit_event(
            &mut guard,
            session_token,
            "rename_path",
            format!("{old_norm} -> {new_norm}").as_str(),
            now,
        );

        Ok(IdeFsResult {
            path: new_norm,
            kind: if old_is_dir {
                "directory".to_string()
            } else {
                "file".to_string()
            },
            version: None,
        })
    }

    pub fn delete_entry(
        &self,
        session_token: &str,
        path: &str,
        write_enabled: bool,
    ) -> Result<IdeFsResult, IdeError> {
        if !write_enabled {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "web IDE authoring is disabled in current runtime mode",
            ));
        }
        let normalized = normalize_workspace_path(path, false)?;
        let resolved = self.resolve_workspace_path(&normalized)?;
        if !resolved.exists() {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "source path not found",
            ));
        }
        let is_dir = resolved.is_dir();

        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_editor_session(&mut guard, session_token, now)?;

        if is_dir {
            std::fs::remove_dir_all(&resolved).map_err(|err| {
                IdeError::new(
                    IdeErrorKind::Internal,
                    format!("delete directory failed: {err}"),
                )
            })?;
        } else {
            std::fs::remove_file(&resolved).map_err(|err| {
                IdeError::new(IdeErrorKind::Internal, format!("delete file failed: {err}"))
            })?;
        }

        if is_dir {
            for key in guard.documents.keys().cloned().collect::<Vec<_>>() {
                if key == normalized || key.starts_with(&format!("{normalized}/")) {
                    guard.documents.remove(&key);
                }
            }
            for session in guard.sessions.values_mut() {
                session.open_paths.retain(|open| {
                    !(open == &normalized || open.starts_with(&format!("{normalized}/")))
                });
            }
        } else {
            guard.documents.remove(&normalized);
            for session in guard.sessions.values_mut() {
                session.open_paths.remove(&normalized);
            }
        }
        guard.analysis_cache.clear();
        self.record_fs_audit_event(
            &mut guard,
            session_token,
            "delete_path",
            normalized.as_str(),
            now,
        );

        Ok(IdeFsResult {
            path: normalized,
            kind: if is_dir {
                "directory".to_string()
            } else {
                "file".to_string()
            },
            version: None,
        })
    }

    pub fn definition(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
        position: Position,
    ) -> Result<Option<IdeLocation>, IdeError> {
        let normalized = normalize_source_path(path)?;
        let context = self.analysis_context(session_token, &normalized, content)?;
        let Some(file_id) = context.file_id_by_path.get(&normalized).copied() else {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "analysis file not found in project context",
            ));
        };
        let Some(source) = context.text_by_file.get(&file_id) else {
            return Err(IdeError::new(
                IdeErrorKind::Internal,
                "analysis source unavailable",
            ));
        };
        let offset = position_to_text_size(source, &position);
        let result = trust_ide::goto_definition(&context.db, file_id, offset);
        Ok(result.and_then(|def| map_definition_location(&context, def)))
    }

    pub fn references(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
        position: Position,
        include_declaration: bool,
    ) -> Result<Vec<IdeLocation>, IdeError> {
        let normalized = normalize_source_path(path)?;
        let context = self.analysis_context(session_token, &normalized, content)?;
        let Some(file_id) = context.file_id_by_path.get(&normalized).copied() else {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "analysis file not found in project context",
            ));
        };
        let Some(source) = context.text_by_file.get(&file_id) else {
            return Err(IdeError::new(
                IdeErrorKind::Internal,
                "analysis source unavailable",
            ));
        };
        let offset = position_to_text_size(source, &position);
        let references = trust_ide::find_references(
            &context.db,
            file_id,
            offset,
            trust_ide::FindReferencesOptions {
                include_declaration,
            },
        );
        Ok(references
            .into_iter()
            .filter_map(|reference| map_reference_location(&context, reference))
            .collect())
    }

    pub fn rename_symbol(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
        position: Position,
        new_name: &str,
        write_enabled: bool,
    ) -> Result<IdeRenameResult, IdeError> {
        if !write_enabled {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "web IDE authoring is disabled in current runtime mode",
            ));
        }
        let normalized = normalize_source_path(path)?;

        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_editor_session(&mut guard, session_token, now)?;

        let context = self.analysis_context_with_guard(
            &mut guard,
            session_token,
            &normalized,
            content.as_deref(),
        )?;
        let Some(file_id) = context.file_id_by_path.get(&normalized).copied() else {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "analysis file not found in project context",
            ));
        };
        let Some(source) = context.text_by_file.get(&file_id) else {
            return Err(IdeError::new(
                IdeErrorKind::Internal,
                "analysis source unavailable",
            ));
        };
        let offset = position_to_text_size(source, &position);
        let rename_result =
            trust_ide::rename(&context.db, file_id, offset, new_name).ok_or_else(|| {
                IdeError::new(
                    IdeErrorKind::InvalidInput,
                    "rename failed for current symbol",
                )
            })?;

        let mut changed = Vec::new();
        for (file_id, edits) in &rename_result.edits {
            let Some(path) = context.path_by_file_id.get(file_id) else {
                continue;
            };
            let original = context
                .text_by_file
                .get(file_id)
                .cloned()
                .unwrap_or_default();
            let updated = apply_text_edits(&original, edits)?;
            if updated.len() > self.limits.max_file_bytes {
                return Err(IdeError::new(
                    IdeErrorKind::TooLarge,
                    format!(
                        "rename result exceeds file limit ({} > {} bytes)",
                        updated.len(),
                        self.limits.max_file_bytes
                    ),
                ));
            }
            let disk_path = self.resolve_source_path(path)?;
            std::fs::write(&disk_path, &updated).map_err(|err| {
                IdeError::new(
                    IdeErrorKind::Internal,
                    format!("rename write failed: {err}"),
                )
            })?;
            let version = {
                let entry =
                    guard
                        .documents
                        .entry(path.clone())
                        .or_insert_with(|| IdeDocumentEntry {
                            content: updated.clone(),
                            version: 1,
                            opened_by: BTreeSet::new(),
                        });
                entry.content = updated;
                entry.version = entry.version.saturating_add(1);
                entry.version
            };
            self.record_fs_audit_event(
                &mut guard,
                session_token,
                "rename_symbol_write",
                path.as_str(),
                now,
            );
            changed.push(IdeWriteResult {
                path: path.clone(),
                version,
            });
        }
        changed.sort_by(|a, b| a.path.cmp(&b.path));
        guard.analysis_cache.clear();

        Ok(IdeRenameResult {
            edit_count: rename_result.edit_count(),
            changed_files: changed,
        })
    }

    pub fn workspace_search(
        &self,
        session_token: &str,
        query: &str,
        include_glob: Option<&str>,
        exclude_glob: Option<&str>,
        limit: usize,
    ) -> Result<Vec<IdeSearchHit>, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);

        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let needle = trimmed.to_ascii_lowercase();
        let include = compile_glob_pattern(include_glob, "include")?;
        let exclude = compile_glob_pattern(exclude_glob, "exclude")?;
        let root = self.workspace_root()?;
        let mut paths = Vec::new();
        collect_workspace_files(&root, &PathBuf::new(), &mut paths)?;
        paths.sort();

        let mut hits = Vec::new();
        for path in paths {
            if include
                .as_ref()
                .is_some_and(|pattern| !pattern.matches(path.as_str()))
            {
                continue;
            }
            if exclude
                .as_ref()
                .is_some_and(|pattern| pattern.matches(path.as_str()))
            {
                continue;
            }
            let source = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
            for (line_idx, line) in source.lines().enumerate() {
                if line.to_ascii_lowercase().contains(&needle) {
                    let byte_idx = line.to_ascii_lowercase().find(&needle).unwrap_or(0);
                    hits.push(IdeSearchHit {
                        path: path.clone(),
                        line: line_idx as u32,
                        character: byte_idx as u32,
                        preview: line.trim().to_string(),
                    });
                    if hits.len() >= limit {
                        return Ok(hits);
                    }
                }
            }
        }
        Ok(hits)
    }

    pub fn workspace_symbols(
        &self,
        session_token: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<IdeSymbolHit>, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);

        let context = self.analysis_context_for_all_files(session_token, None)?;
        Ok(extract_symbol_hits(&context, None, query, limit))
    }

    pub fn file_symbols(
        &self,
        session_token: &str,
        path: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<IdeSymbolHit>, IdeError> {
        let normalized = normalize_source_path(path)?;
        let context = self.analysis_context(session_token, &normalized, None)?;
        Ok(extract_symbol_hits(
            &context,
            Some(&normalized),
            query,
            limit,
        ))
    }

    pub fn open_source(
        &self,
        session_token: &str,
        path: &str,
    ) -> Result<IdeFileSnapshot, IdeError> {
        let normalized = normalize_workspace_file_path(path)?;
        let source_path = self.resolve_source_path(&normalized)?;
        let disk_content = std::fs::read_to_string(&source_path)
            .map_err(|_| IdeError::new(IdeErrorKind::NotFound, "source file not found"))?;
        if disk_content.len() > self.limits.max_file_bytes {
            return Err(IdeError::new(
                IdeErrorKind::TooLarge,
                format!(
                    "source file exceeds limit ({} > {} bytes)",
                    disk_content.len(),
                    self.limits.max_file_bytes
                ),
            ));
        }

        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let role = {
            let session = self.ensure_session(&mut guard, session_token, now)?;
            session.open_paths.insert(normalized.clone());
            session.role
        };

        let entry = guard
            .documents
            .entry(normalized.clone())
            .or_insert_with(|| IdeDocumentEntry {
                content: disk_content.clone(),
                version: 1,
                opened_by: BTreeSet::new(),
            });
        if entry.content != disk_content {
            entry.content = disk_content.clone();
            entry.version = entry.version.saturating_add(1);
        }
        entry.opened_by.insert(session_token.to_string());

        Ok(IdeFileSnapshot {
            path: normalized,
            content: disk_content,
            version: entry.version,
            read_only: !matches!(role, IdeRole::Editor),
        })
    }

    pub fn apply_source(
        &self,
        session_token: &str,
        path: &str,
        expected_version: u64,
        content: String,
        write_enabled: bool,
    ) -> Result<IdeWriteResult, IdeError> {
        if !write_enabled {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "web IDE authoring is disabled in current runtime mode",
            ));
        }
        if content.len() > self.limits.max_file_bytes {
            return Err(IdeError::new(
                IdeErrorKind::TooLarge,
                format!(
                    "payload exceeds limit ({} > {} bytes)",
                    content.len(),
                    self.limits.max_file_bytes
                ),
            ));
        }

        let normalized = normalize_workspace_file_path(path)?;
        let source_path = self.resolve_source_path(&normalized)?;
        let disk_content = std::fs::read_to_string(&source_path)
            .map_err(|_| IdeError::new(IdeErrorKind::NotFound, "source file not found"))?;

        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let role = {
            let session = self.ensure_session(&mut guard, session_token, now)?;
            session.open_paths.insert(normalized.clone());
            session.role
        };
        if !matches!(role, IdeRole::Editor) {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "session role does not allow edits",
            ));
        }

        let next_version = {
            let entry = guard
                .documents
                .entry(normalized.clone())
                .or_insert_with(|| IdeDocumentEntry {
                    content: disk_content.clone(),
                    version: 1,
                    opened_by: BTreeSet::new(),
                });
            if entry.content != disk_content {
                entry.content = disk_content;
                entry.version = entry.version.saturating_add(1);
            }
            if entry.version != expected_version {
                return Err(IdeError::conflict(entry.version));
            }

            std::fs::write(&source_path, &content).map_err(|err| {
                IdeError::new(IdeErrorKind::Internal, format!("write failed: {err}"))
            })?;

            entry.content = content;
            entry.version = entry.version.saturating_add(1);
            entry.opened_by.insert(session_token.to_string());
            entry.version
        };
        self.record_fs_audit_event(
            &mut guard,
            session_token,
            "write_file",
            normalized.as_str(),
            now,
        );
        guard.analysis_cache.clear();

        Ok(IdeWriteResult {
            path: normalized,
            version: next_version,
        })
    }

    pub fn format_source(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
    ) -> Result<IdeFormatResult, IdeError> {
        let normalized = normalize_source_path(path)?;
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);

        let current = if let Some(content) = content {
            content
        } else {
            let source_path = self.resolve_source_path(&normalized)?;
            std::fs::read_to_string(&source_path)
                .map_err(|_| IdeError::new(IdeErrorKind::NotFound, "source file not found"))?
        };
        if current.len() > self.limits.max_file_bytes {
            return Err(IdeError::new(
                IdeErrorKind::TooLarge,
                format!(
                    "source file exceeds limit ({} > {} bytes)",
                    current.len(),
                    self.limits.max_file_bytes
                ),
            ));
        }
        let formatted = format_structured_text_document(current.as_str());
        let changed = formatted != current;
        Ok(IdeFormatResult {
            path: normalized,
            content: formatted,
            changed,
        })
    }

    pub fn diagnostics(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
    ) -> Result<Vec<DiagnosticItem>, IdeError> {
        let normalized = normalize_source_path(path)?;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_analysis_cache(&mut guard, session_token, &normalized, content.as_deref())?;
        let uri = format!("memory:///{normalized}");
        let entry = guard.analysis_cache.get_mut(session_token).ok_or_else(|| {
            IdeError::new(IdeErrorKind::Internal, "analysis cache missing for session")
        })?;
        entry.engine.diagnostics(&uri).map_err(map_analysis_error)
    }

    pub fn hover(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
        position: Position,
    ) -> Result<Option<HoverItem>, IdeError> {
        let normalized = normalize_source_path(path)?;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_analysis_cache(&mut guard, session_token, &normalized, content.as_deref())?;
        let uri = format!("memory:///{normalized}");
        let entry = guard.analysis_cache.get_mut(session_token).ok_or_else(|| {
            IdeError::new(IdeErrorKind::Internal, "analysis cache missing for session")
        })?;
        entry
            .engine
            .hover(HoverRequest { uri, position })
            .map_err(map_analysis_error)
    }

    pub fn completion(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
        position: Position,
        limit: Option<u32>,
    ) -> Result<Vec<CompletionItem>, IdeError> {
        let normalized = normalize_source_path(path)?;
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.ensure_analysis_cache(&mut guard, session_token, &normalized, content.as_deref())?;
        let uri = format!("memory:///{normalized}");
        let entry = guard.analysis_cache.get_mut(session_token).ok_or_else(|| {
            IdeError::new(IdeErrorKind::Internal, "analysis cache missing for session")
        })?;
        let active_text = entry.docs.get(&normalized).cloned().unwrap_or_default();
        let mut result = entry
            .engine
            .completion(CompletionRequest {
                uri,
                position: position.clone(),
                limit,
            })
            .map_err(map_analysis_error)?;
        apply_completion_relevance_contract(&mut result, &active_text, position, limit);
        Ok(result)
    }

    pub fn health(&self, session_token: &str) -> Result<WebIdeHealth, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;

        let editor_sessions = guard
            .sessions
            .values()
            .filter(|entry| matches!(entry.role, IdeRole::Editor))
            .count();
        let open_document_handles = guard
            .documents
            .values()
            .map(|entry| entry.opened_by.len())
            .sum::<usize>();
        let frontend_telemetry = guard.frontend_telemetry_by_session.values().fold(
            WebIdeFrontendTelemetry::default(),
            |mut agg, item| {
                agg.bootstrap_failures = agg
                    .bootstrap_failures
                    .saturating_add(item.bootstrap_failures);
                agg.analysis_timeouts =
                    agg.analysis_timeouts.saturating_add(item.analysis_timeouts);
                agg.worker_restarts = agg.worker_restarts.saturating_add(item.worker_restarts);
                agg.autosave_failures =
                    agg.autosave_failures.saturating_add(item.autosave_failures);
                agg
            },
        );

        Ok(WebIdeHealth {
            active_sessions: guard.sessions.len(),
            editor_sessions,
            tracked_documents: guard.documents.len(),
            open_document_handles,
            fs_mutation_events: guard.fs_audit_log.len(),
            limits: self.limits.clone(),
            frontend_telemetry,
        })
    }

    pub fn browse_directory(
        &self,
        session_token: &str,
        path: Option<&str>,
    ) -> Result<IdeBrowseResult, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;
        drop(guard);

        let dir = match path {
            Some(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
            _ => std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/")),
        };

        let canonical = dir.canonicalize().map_err(|err| {
            IdeError::new(
                IdeErrorKind::NotFound,
                format!("cannot resolve path: {err}"),
            )
        })?;

        // Block sensitive system paths.
        let display = canonical.to_string_lossy();
        for blocked in &["/proc", "/sys", "/dev"] {
            if display.starts_with(blocked) {
                return Err(IdeError::new(
                    IdeErrorKind::Forbidden,
                    format!("browsing {blocked} is not allowed"),
                ));
            }
        }

        if !canonical.is_dir() {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "path is not a directory",
            ));
        }

        let read = std::fs::read_dir(&canonical).map_err(|err| {
            IdeError::new(
                IdeErrorKind::Forbidden,
                format!("cannot read directory: {err}"),
            )
        })?;

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let entry_path = entry.path();
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            let size = meta.as_ref().map_or(0, |m| m.len());
            let item = IdeBrowseEntry {
                name: name.clone(),
                path: entry_path.to_string_lossy().to_string(),
                kind: if is_dir {
                    "directory".to_string()
                } else {
                    "file".to_string()
                },
                size,
            };
            if is_dir {
                dirs.push(item);
            } else {
                files.push(item);
            }
        }

        dirs.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });
        files.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });
        dirs.append(&mut files);

        let parent_path = canonical.parent().map(|p| p.to_string_lossy().to_string());

        Ok(IdeBrowseResult {
            current_path: canonical.to_string_lossy().to_string(),
            parent_path,
            entries: dirs,
        })
    }

    pub fn fs_audit(
        &self,
        session_token: &str,
        limit: usize,
    ) -> Result<Vec<IdeFsAuditRecord>, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;
        let take = limit.clamp(1, 200);
        let events = guard
            .fs_audit_log
            .iter()
            .rev()
            .take(take)
            .map(|event| IdeFsAuditRecord {
                ts_secs: event.ts_secs,
                session: event.session.clone(),
                action: event.action.clone(),
                path: event.path.clone(),
            })
            .collect();
        Ok(events)
    }

    pub fn record_frontend_telemetry(
        &self,
        session_token: &str,
        report: WebIdeFrontendTelemetry,
    ) -> Result<WebIdeFrontendTelemetry, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;
        guard
            .frontend_telemetry_by_session
            .insert(session_token.to_string(), report);
        let aggregated = guard.frontend_telemetry_by_session.values().fold(
            WebIdeFrontendTelemetry::default(),
            |mut agg, item| {
                agg.bootstrap_failures = agg
                    .bootstrap_failures
                    .saturating_add(item.bootstrap_failures);
                agg.analysis_timeouts =
                    agg.analysis_timeouts.saturating_add(item.analysis_timeouts);
                agg.worker_restarts = agg.worker_restarts.saturating_add(item.worker_restarts);
                agg.autosave_failures =
                    agg.autosave_failures.saturating_add(item.autosave_failures);
                agg
            },
        );
        Ok(aggregated)
    }

    fn ensure_analysis_cache(
        &self,
        guard: &mut IdeStateInner,
        session_token: &str,
        active_path: &str,
        content_override: Option<&str>,
    ) -> Result<(), IdeError> {
        let now = (self.now)();
        let _ = self.ensure_session(guard, session_token, now)?;
        let root = self.workspace_root()?;

        let cache_key = session_token.to_string();
        let mut cache = guard.analysis_cache.remove(&cache_key).unwrap_or_default();
        let result = (|| -> Result<(), IdeError> {
            let refresh_due = !cache.initialized
                || now >= cache.next_refresh_at_secs
                || !cache.docs.contains_key(active_path);
            let mut docs_changed = false;

            if refresh_due {
                let mut files = Vec::new();
                collect_source_files(&root, &PathBuf::new(), &mut files)?;
                files.sort();

                let mut seen = BTreeSet::new();
                for rel_path in files {
                    let normalized = normalize_source_path(&rel_path)?;
                    seen.insert(normalized.clone());
                    let disk_path = root.join(&normalized);
                    let fingerprint = match source_fingerprint(&disk_path) {
                        Ok(value) => value,
                        Err(error) => {
                            if normalized == active_path {
                                return Err(error);
                            }
                            if cache.docs.remove(&normalized).is_some() {
                                docs_changed = true;
                            }
                            cache.fingerprints.remove(&normalized);
                            guard.documents.remove(&normalized);
                            continue;
                        }
                    };
                    let needs_reload = cache.fingerprints.get(&normalized).copied()
                        != Some(fingerprint)
                        || !cache.docs.contains_key(&normalized);

                    if needs_reload {
                        let text =
                            match read_source_with_limit(&disk_path, self.limits.max_file_bytes) {
                                Ok(value) => value,
                                Err(error) => {
                                    if normalized == active_path {
                                        return Err(error);
                                    }
                                    if cache.docs.remove(&normalized).is_some() {
                                        docs_changed = true;
                                    }
                                    cache.fingerprints.remove(&normalized);
                                    guard.documents.remove(&normalized);
                                    continue;
                                }
                            };
                        if cache.docs.get(&normalized).map(String::as_str) != Some(text.as_str()) {
                            docs_changed = true;
                        }
                        cache.docs.insert(normalized.clone(), text.clone());
                        cache.fingerprints.insert(normalized.clone(), fingerprint);
                        Self::upsert_tracked_document(guard, session_token, &normalized, text);
                    } else if let Some(existing) = cache.docs.get(&normalized).cloned() {
                        Self::upsert_tracked_document(guard, session_token, &normalized, existing);
                    }
                }

                let stale_paths = cache
                    .docs
                    .keys()
                    .filter(|path| !seen.contains(*path))
                    .cloned()
                    .collect::<Vec<_>>();
                if !stale_paths.is_empty() {
                    docs_changed = true;
                }
                for stale in stale_paths {
                    cache.docs.remove(&stale);
                    cache.fingerprints.remove(&stale);
                    guard.documents.remove(&stale);
                }

                cache.initialized = true;
                cache.next_refresh_at_secs =
                    now.saturating_add(ANALYSIS_CACHE_REFRESH_INTERVAL_SECS);
            }

            if let Some(override_text) = content_override {
                if override_text.len() > self.limits.max_file_bytes {
                    return Err(IdeError::new(
                        IdeErrorKind::TooLarge,
                        format!(
                            "source file exceeds limit ({} > {} bytes)",
                            override_text.len(),
                            self.limits.max_file_bytes
                        ),
                    ));
                }
                let Some(existing) = cache.docs.get_mut(active_path) else {
                    return Err(IdeError::new(
                        IdeErrorKind::NotFound,
                        "analysis file not found in project context",
                    ));
                };
                if existing != override_text {
                    *existing = override_text.to_string();
                    docs_changed = true;
                }
                Self::upsert_tracked_document(
                    guard,
                    session_token,
                    active_path,
                    override_text.to_string(),
                );
            } else if let Some(existing) = cache.docs.get(active_path).cloned() {
                Self::upsert_tracked_document(guard, session_token, active_path, existing);
            }

            if !cache.docs.contains_key(active_path) {
                return Err(IdeError::new(
                    IdeErrorKind::NotFound,
                    "analysis file not found in project context",
                ));
            }

            if docs_changed || !cache.engine_applied {
                let documents = cache
                    .docs
                    .iter()
                    .map(|(path, text)| DocumentInput {
                        uri: format!("memory:///{path}"),
                        text: text.clone(),
                    })
                    .collect::<Vec<_>>();
                cache
                    .engine
                    .replace_documents(documents)
                    .map_err(map_analysis_error)?;
                cache.engine_applied = true;
            }

            Ok(())
        })();
        guard.analysis_cache.insert(cache_key, cache);
        result
    }

    fn upsert_tracked_document(
        guard: &mut IdeStateInner,
        session_token: &str,
        path: &str,
        content: String,
    ) {
        let entry = guard
            .documents
            .entry(path.to_string())
            .or_insert_with(|| IdeDocumentEntry {
                content: content.clone(),
                version: 1,
                opened_by: BTreeSet::new(),
            });
        if entry.content != content {
            entry.content = content;
            entry.version = entry.version.saturating_add(1);
        }
        entry.opened_by.insert(session_token.to_string());
    }

    fn analysis_context(
        &self,
        session_token: &str,
        path: &str,
        content: Option<String>,
    ) -> Result<AnalysisContext, IdeError> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        self.analysis_context_with_guard(&mut guard, session_token, path, content.as_deref())
    }

    fn analysis_context_for_all_files(
        &self,
        session_token: &str,
        content_override: Option<(&str, &str)>,
    ) -> Result<AnalysisContext, IdeError> {
        let now = (self.now)();
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| IdeError::new(IdeErrorKind::Internal, "session state lock poisoned"))?;
        let _ = self.ensure_session(&mut guard, session_token, now)?;
        self.build_analysis_context(&mut guard, session_token, content_override)
    }

    fn build_analysis_context(
        &self,
        guard: &mut IdeStateInner,
        session_token: &str,
        content_override: Option<(&str, &str)>,
    ) -> Result<AnalysisContext, IdeError> {
        let root = self.workspace_root()?;
        let mut files = Vec::new();
        collect_source_files(&root, &PathBuf::new(), &mut files)?;
        files.sort();

        let mut db = trust_hir::Database::new();
        let mut file_id_by_path = BTreeMap::new();
        let mut path_by_file_id = HashMap::new();
        let mut text_by_file = HashMap::new();

        for (index, rel_path) in files.iter().enumerate() {
            let normalized = normalize_source_path(rel_path)?;
            let disk_path = self.resolve_source_path(&normalized)?;
            let mut text = std::fs::read_to_string(&disk_path)
                .map_err(|_| IdeError::new(IdeErrorKind::NotFound, "source file not found"))?;
            if let Some((override_path, override_text)) = content_override {
                if normalized == override_path {
                    text = override_text.to_string();
                }
            }
            if text.len() > self.limits.max_file_bytes {
                return Err(IdeError::new(
                    IdeErrorKind::TooLarge,
                    format!(
                        "source file exceeds limit ({} > {} bytes)",
                        text.len(),
                        self.limits.max_file_bytes
                    ),
                ));
            }
            let file_id = FileId(index as u32);
            db.set_source_text(file_id, text.clone());
            file_id_by_path.insert(normalized.clone(), file_id);
            path_by_file_id.insert(file_id, normalized.clone());
            text_by_file.insert(file_id, text.clone());

            let entry = guard
                .documents
                .entry(normalized.clone())
                .or_insert_with(|| IdeDocumentEntry {
                    content: text.clone(),
                    version: 1,
                    opened_by: BTreeSet::new(),
                });
            if entry.content != text {
                entry.content = text;
                entry.version = entry.version.saturating_add(1);
            }
            entry.opened_by.insert(session_token.to_string());
        }

        Ok(AnalysisContext {
            db,
            file_id_by_path,
            path_by_file_id,
            text_by_file,
        })
    }

    fn analysis_context_with_guard(
        &self,
        guard: &mut IdeStateInner,
        session_token: &str,
        path: &str,
        content: Option<&str>,
    ) -> Result<AnalysisContext, IdeError> {
        let now = (self.now)();
        let _ = self.ensure_session(guard, session_token, now)?;
        self.build_analysis_context(guard, session_token, content.map(|text| (path, text)))
    }

    fn ensure_session<'a>(
        &self,
        guard: &'a mut IdeStateInner,
        session_token: &str,
        now: u64,
    ) -> Result<&'a mut IdeSessionEntry, IdeError> {
        prune_expired(guard, now);
        let session = guard.sessions.get_mut(session_token).ok_or_else(|| {
            IdeError::new(IdeErrorKind::Unauthorized, "invalid or expired session")
        })?;
        // Sliding renewal keeps active sessions alive while preserving TTL for idle sessions.
        session.expires_at = now.saturating_add(self.limits.session_ttl_secs);
        Ok(session)
    }

    fn ensure_editor_session<'a>(
        &self,
        guard: &'a mut IdeStateInner,
        session_token: &str,
        now: u64,
    ) -> Result<&'a mut IdeSessionEntry, IdeError> {
        let session = self.ensure_session(guard, session_token, now)?;
        if !matches!(session.role, IdeRole::Editor) {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "session role does not allow edits",
            ));
        }
        Ok(session)
    }

    fn record_fs_audit_event(
        &self,
        guard: &mut IdeStateInner,
        session_token: &str,
        action: &str,
        path: &str,
        ts_secs: u64,
    ) {
        let session = session_token.chars().take(8).collect::<String>();
        guard.fs_audit_log.push(IdeFsAuditEvent {
            ts_secs,
            session,
            action: action.to_string(),
            path: path.to_string(),
        });
        if guard.fs_audit_log.len() > MAX_FS_AUDIT_EVENTS {
            let drain = guard.fs_audit_log.len() - MAX_FS_AUDIT_EVENTS;
            guard.fs_audit_log.drain(0..drain);
        }
    }

    fn resolve_source_path(&self, normalized: &str) -> Result<PathBuf, IdeError> {
        self.resolve_workspace_path(normalized)
    }

    fn resolve_workspace_path(&self, normalized: &str) -> Result<PathBuf, IdeError> {
        let root = self.workspace_root()?;
        let joined = root.join(normalized);
        let canonical_root = root.canonicalize().unwrap_or(root.clone());
        let canonical_parent = closest_existing_parent(joined.parent(), &canonical_root)?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(IdeError::new(
                IdeErrorKind::Forbidden,
                "workspace path escapes project root",
            ));
        }
        Ok(joined)
    }

    fn workspace_root(&self) -> Result<PathBuf, IdeError> {
        let Some(root) = self.active_project_root() else {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "project root unavailable for web IDE",
            ));
        };
        if !root.is_dir() {
            return Err(IdeError::new(
                IdeErrorKind::NotFound,
                "project root directory is missing",
            ));
        }
        Ok(root)
    }
}

#[derive(Debug)]
struct AnalysisContext {
    db: trust_hir::Database,
    file_id_by_path: BTreeMap<String, FileId>,
    path_by_file_id: HashMap<FileId, String>,
    text_by_file: HashMap<FileId, String>,
}

fn normalize_source_path(path: &str) -> Result<String, IdeError> {
    let normalized = normalize_workspace_path(path, false)?;
    if !normalized.to_ascii_lowercase().ends_with(".st") {
        return Err(IdeError::new(
            IdeErrorKind::InvalidInput,
            "only .st files are allowed",
        ));
    }
    Ok(normalized)
}

fn normalize_workspace_file_path(path: &str) -> Result<String, IdeError> {
    normalize_workspace_path(path, false)
}

fn normalize_workspace_path(path: &str, allow_root: bool) -> Result<String, IdeError> {
    let trimmed = path.trim();
    if trimmed.is_empty() && !allow_root {
        return Err(IdeError::new(
            IdeErrorKind::InvalidInput,
            "workspace path is required",
        ));
    }
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        return Err(IdeError::new(
            IdeErrorKind::Forbidden,
            "absolute workspace paths are not allowed",
        ));
    }

    let mut parts = Vec::new();
    for component in raw.components() {
        match component {
            Component::Normal(value) => {
                let text = value.to_string_lossy();
                if text.starts_with('.') {
                    return Err(IdeError::new(
                        IdeErrorKind::Forbidden,
                        "hidden workspace paths are not allowed",
                    ));
                }
                parts.push(text.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(IdeError::new(
                    IdeErrorKind::Forbidden,
                    "workspace path escapes project root",
                ));
            }
        }
    }

    if parts.is_empty() {
        return Err(IdeError::new(
            IdeErrorKind::InvalidInput,
            "workspace path is required",
        ));
    }

    Ok(parts.join("/"))
}

fn normalize_project_root(path: &str) -> Result<PathBuf, IdeError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(IdeError::new(
            IdeErrorKind::InvalidInput,
            "project root path is required",
        ));
    }
    let raw = PathBuf::from(trimmed);
    let absolute = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(raw)
    };
    Ok(absolute)
}

fn pathbuf_to_display(path: PathBuf) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn closest_existing_parent(
    mut cursor: Option<&Path>,
    canonical_root: &Path,
) -> Result<PathBuf, IdeError> {
    while let Some(path) = cursor {
        if path.exists() {
            return path
                .canonicalize()
                .map_err(|_| IdeError::new(IdeErrorKind::NotFound, "workspace folder not found"));
        }
        cursor = path.parent();
    }
    Ok(canonical_root.to_path_buf())
}

fn source_fingerprint(path: &Path) -> Result<SourceFingerprint, IdeError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| IdeError::new(IdeErrorKind::NotFound, "source file not found"))?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis());
    Ok(SourceFingerprint {
        size_bytes: metadata.len(),
        modified_ms,
    })
}

fn read_source_with_limit(path: &Path, max_file_bytes: usize) -> Result<String, IdeError> {
    let text = std::fs::read_to_string(path)
        .map_err(|_| IdeError::new(IdeErrorKind::NotFound, "source file not found"))?;
    if text.len() > max_file_bytes {
        return Err(IdeError::new(
            IdeErrorKind::TooLarge,
            format!(
                "source file exceeds limit ({} > {} bytes)",
                text.len(),
                max_file_bytes
            ),
        ));
    }
    Ok(text)
}

fn compile_glob_pattern(raw: Option<&str>, field: &str) -> Result<Option<Pattern>, IdeError> {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    Pattern::new(trimmed).map(Some).map_err(|err| {
        IdeError::new(
            IdeErrorKind::InvalidInput,
            format!("invalid {field} glob pattern '{trimmed}': {err}"),
        )
    })
}

fn map_definition_location(
    context: &AnalysisContext,
    result: trust_ide::DefinitionResult,
) -> Option<IdeLocation> {
    let path = context.path_by_file_id.get(&result.file_id)?.clone();
    let text = context.text_by_file.get(&result.file_id)?;
    Some(IdeLocation {
        path,
        range: text_range_to_ide_range(text, result.range),
    })
}

fn map_reference_location(
    context: &AnalysisContext,
    reference: trust_ide::Reference,
) -> Option<IdeLocation> {
    let path = context.path_by_file_id.get(&reference.file_id)?.clone();
    let text = context.text_by_file.get(&reference.file_id)?;
    Some(IdeLocation {
        path,
        range: text_range_to_ide_range(text, reference.range),
    })
}

fn text_range_to_ide_range(text: &str, range: TextRange) -> IdeRange {
    IdeRange {
        start: text_offset_to_position(text, range.start()),
        end: text_offset_to_position(text, range.end()),
    }
}

fn position_to_text_size(text: &str, position: &Position) -> TextSize {
    let line_idx = position.line as usize;
    let char_idx = position.character as usize;
    let mut start = 0usize;
    for (current, line) in text.split('\n').enumerate() {
        if current == line_idx {
            let mut byte_in_line = line.len();
            if char_idx == 0 {
                byte_in_line = 0;
            } else {
                for (count, (idx, _)) in line.char_indices().enumerate() {
                    if count == char_idx {
                        byte_in_line = idx;
                        break;
                    }
                }
            }
            return TextSize::from((start + byte_in_line) as u32);
        }
        start = start.saturating_add(line.len() + 1);
    }
    TextSize::from(text.len() as u32)
}

fn text_offset_to_position(text: &str, offset: TextSize) -> IdePosition {
    let offset = u32::from(offset) as usize;
    let safe_offset = offset.min(text.len());
    let prefix = &text[..safe_offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let char_idx = prefix
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count())
        .unwrap_or_else(|| prefix.chars().count());
    IdePosition {
        line: line as u32,
        character: char_idx as u32,
    }
}

fn apply_text_edits(text: &str, edits: &[trust_ide::rename::TextEdit]) -> Result<String, IdeError> {
    let mut sorted = edits.to_vec();
    sorted.sort_by(|a, b| b.range.start().cmp(&a.range.start()));

    let mut output = text.to_string();
    for edit in sorted {
        let start = usize::try_from(u32::from(edit.range.start())).map_err(|_| {
            IdeError::new(
                IdeErrorKind::InvalidInput,
                "invalid rename edit range start",
            )
        })?;
        let end = usize::try_from(u32::from(edit.range.end())).map_err(|_| {
            IdeError::new(IdeErrorKind::InvalidInput, "invalid rename edit range end")
        })?;
        if start > end || end > output.len() {
            return Err(IdeError::new(
                IdeErrorKind::InvalidInput,
                "rename edit range out of bounds",
            ));
        }
        output.replace_range(start..end, edit.new_text.as_str());
    }
    Ok(output)
}

fn symbol_kind_label(kind: &trust_hir::symbols::SymbolKind) -> &'static str {
    use trust_hir::symbols::SymbolKind;
    match kind {
        SymbolKind::Program => "program",
        SymbolKind::Configuration => "configuration",
        SymbolKind::Resource => "resource",
        SymbolKind::Task => "task",
        SymbolKind::ProgramInstance => "program_instance",
        SymbolKind::Namespace => "namespace",
        SymbolKind::Function { .. } => "function",
        SymbolKind::FunctionBlock => "function_block",
        SymbolKind::Class => "class",
        SymbolKind::Method { .. } => "method",
        SymbolKind::Property { .. } => "property",
        SymbolKind::Interface => "interface",
        SymbolKind::Variable { .. } => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::Type => "type",
        SymbolKind::EnumValue { .. } => "enum_value",
        SymbolKind::Parameter { .. } => "parameter",
    }
}

fn extract_symbol_hits(
    context: &AnalysisContext,
    filter_path: Option<&str>,
    query: &str,
    limit: usize,
) -> Vec<IdeSymbolHit> {
    let query = query.trim().to_ascii_lowercase();
    let mut hits = Vec::new();
    for (path, file_id) in &context.file_id_by_path {
        if let Some(expected) = filter_path {
            if path != expected {
                continue;
            }
        }
        let symbols = context.db.file_symbols(*file_id);
        let Some(source) = context.text_by_file.get(file_id) else {
            continue;
        };
        for symbol in symbols.iter() {
            if symbol.name.is_empty() {
                continue;
            }
            if !query.is_empty() && !symbol.name.to_ascii_lowercase().contains(&query) {
                continue;
            }
            let pos = text_offset_to_position(source, symbol.range.start());
            hits.push(IdeSymbolHit {
                path: path.clone(),
                name: symbol.name.to_string(),
                kind: symbol_kind_label(&symbol.kind).to_string(),
                line: pos.line,
                character: pos.character,
            });
            if hits.len() >= limit {
                return hits;
            }
        }
    }
    hits
}

fn map_analysis_error(error: impl std::fmt::Display) -> IdeError {
    IdeError::new(
        IdeErrorKind::InvalidInput,
        format!("analysis error: {error}"),
    )
}

fn apply_completion_relevance_contract(
    items: &mut Vec<CompletionItem>,
    text: &str,
    position: Position,
    limit: Option<u32>,
) {
    let prefix = completion_prefix(text, position);
    if prefix.is_empty() {
        if let Some(max) = limit {
            items.truncate(max as usize);
        }
        return;
    }
    let prefix_lower = prefix.to_ascii_lowercase();

    let mut seen_labels: BTreeSet<String> = items
        .iter()
        .map(|item| item.label.to_ascii_lowercase())
        .collect();
    let mut fallback_symbols = extract_in_scope_symbols(text)
        .into_iter()
        .filter(|symbol| {
            let lowered = symbol.to_ascii_lowercase();
            lowered.starts_with(&prefix_lower) && !seen_labels.contains(&lowered)
        })
        .collect::<Vec<_>>();
    fallback_symbols.sort();

    if !fallback_symbols.is_empty() {
        let mut prefixed = Vec::with_capacity(fallback_symbols.len() + items.len());
        for symbol in fallback_symbols {
            seen_labels.insert(symbol.to_ascii_lowercase());
            prefixed.push(CompletionItem {
                label: symbol.clone(),
                kind: "symbol".to_string(),
                detail: Some("in-scope symbol".to_string()),
                documentation: None,
                insert_text: Some(symbol),
                text_edit: None,
                sort_priority: 0,
            });
        }
        prefixed.append(items);
        *items = prefixed;
    }

    items.sort_by(|a, b| {
        let rank_a = completion_rank(a, &prefix_lower);
        let rank_b = completion_rank(b, &prefix_lower);
        rank_a
            .cmp(&rank_b)
            .then(a.sort_priority.cmp(&b.sort_priority))
            .then(a.label.cmp(&b.label))
    });

    let mut deduped = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for item in items.drain(..) {
        let key = item.label.to_ascii_lowercase();
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    if let Some(max) = limit {
        deduped.truncate(max as usize);
    }
    *items = deduped;
}

fn completion_rank(item: &CompletionItem, prefix_lower: &str) -> u8 {
    let label_lower = item.label.to_ascii_lowercase();
    if label_lower.starts_with(prefix_lower) {
        let kind_lower = item.kind.to_ascii_lowercase();
        if kind_lower == "keyword" {
            return 1;
        }
        return 0;
    }
    2
}

fn completion_prefix(text: &str, position: Position) -> String {
    let line = text
        .split('\n')
        .nth(position.line as usize)
        .unwrap_or_default();
    let mut char_to_byte = 0_usize;
    for (count, (idx, _ch)) in line.char_indices().enumerate() {
        if count == position.character as usize {
            char_to_byte = idx;
            break;
        }
        char_to_byte = line.len();
    }
    if (position.character as usize) == 0 {
        char_to_byte = 0;
    } else if (position.character as usize) >= line.chars().count() {
        char_to_byte = line.len();
    }
    let before = &line[..char_to_byte];
    let start = before
        .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    before[start..].trim().to_string()
}

fn extract_in_scope_symbols(text: &str) -> BTreeSet<String> {
    let mut symbols = BTreeSet::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with("(*") {
            continue;
        }
        for keyword in ["PROGRAM", "FUNCTION", "FUNCTION_BLOCK", "TYPE", "CLASS"] {
            if let Some(rest) = line.strip_prefix(keyword) {
                let candidate = rest.split_whitespace().next().unwrap_or_default();
                if is_identifier(candidate) {
                    symbols.insert(candidate.to_string());
                }
            }
        }
        if let Some((lhs, _rhs)) = line.split_once(':') {
            for part in lhs.split(',') {
                let candidate = part
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches(';');
                if is_identifier(candidate) {
                    symbols.insert(candidate.to_string());
                }
            }
        }
    }
    symbols
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn format_structured_text_document(source: &str) -> String {
    let ends_with_newline = source.ends_with('\n');
    let mut indent_level = 0_usize;
    let mut out_lines = Vec::new();

    for raw_line in source.lines() {
        let line_no_trailing = raw_line.trim_end_matches([' ', '\t']);
        let trimmed = line_no_trailing.trim_start();
        if trimmed.is_empty() {
            out_lines.push(String::new());
            continue;
        }
        if trimmed.starts_with("//") || trimmed.starts_with("(*") {
            out_lines.push(format!("{}{}", "  ".repeat(indent_level), trimmed));
            continue;
        }

        let upper = trimmed.to_ascii_uppercase();
        let dedent_before = is_dedent_line(upper.as_str());
        if dedent_before && indent_level > 0 {
            indent_level = indent_level.saturating_sub(1);
        }

        out_lines.push(format!("{}{}", "  ".repeat(indent_level), trimmed));

        if is_indent_line(upper.as_str()) {
            indent_level = indent_level.saturating_add(1);
        }
    }

    let mut formatted = out_lines.join("\n");
    if !formatted.is_empty() && (ends_with_newline || !source.is_empty()) {
        formatted.push('\n');
    }
    formatted
}

fn is_dedent_line(upper_trimmed: &str) -> bool {
    if upper_trimmed.starts_with("END_") {
        return true;
    }
    upper_trimmed == "ELSE"
        || upper_trimmed.starts_with("ELSE ")
        || upper_trimmed.starts_with("ELSIF ")
        || upper_trimmed.starts_with("UNTIL ")
}

fn is_indent_line(upper_trimmed: &str) -> bool {
    if upper_trimmed.starts_with("PROGRAM ")
        || upper_trimmed.starts_with("FUNCTION ")
        || upper_trimmed.starts_with("FUNCTION_BLOCK ")
        || upper_trimmed.starts_with("CONFIGURATION ")
        || upper_trimmed.starts_with("RESOURCE ")
        || upper_trimmed.starts_with("CLASS ")
        || upper_trimmed.starts_with("INTERFACE ")
        || upper_trimmed.starts_with("METHOD ")
        || upper_trimmed.starts_with("PROPERTY ")
        || upper_trimmed.starts_with("ACTION ")
        || upper_trimmed.starts_with("TRANSITION ")
        || upper_trimmed == "ELSE"
        || upper_trimmed.starts_with("ELSE ")
        || upper_trimmed.starts_with("ELSIF ")
        || upper_trimmed.starts_with("REPEAT")
    {
        return true;
    }
    if upper_trimmed == "VAR"
        || upper_trimmed.starts_with("VAR ")
        || upper_trimmed == "VAR_INPUT"
        || upper_trimmed.starts_with("VAR_INPUT ")
        || upper_trimmed == "VAR_OUTPUT"
        || upper_trimmed.starts_with("VAR_OUTPUT ")
        || upper_trimmed == "VAR_IN_OUT"
        || upper_trimmed.starts_with("VAR_IN_OUT ")
        || upper_trimmed == "VAR_TEMP"
        || upper_trimmed.starts_with("VAR_TEMP ")
        || upper_trimmed == "VAR_GLOBAL"
        || upper_trimmed.starts_with("VAR_GLOBAL ")
        || upper_trimmed == "VAR_EXTERNAL"
        || upper_trimmed.starts_with("VAR_EXTERNAL ")
        || upper_trimmed == "VAR_CONFIG"
        || upper_trimmed.starts_with("VAR_CONFIG ")
        || upper_trimmed == "VAR_ACCESS"
        || upper_trimmed.starts_with("VAR_ACCESS ")
    {
        return true;
    }
    (upper_trimmed.starts_with("IF ") && upper_trimmed.contains(" THEN"))
        || (upper_trimmed.starts_with("CASE ") && upper_trimmed.contains(" OF"))
        || (upper_trimmed.starts_with("FOR ") && upper_trimmed.contains(" DO"))
        || (upper_trimmed.starts_with("WHILE ") && upper_trimmed.contains(" DO"))
}

fn collect_workspace_files(
    root: &Path,
    relative: &Path,
    out: &mut Vec<String>,
) -> Result<(), IdeError> {
    let dir = root.join(relative);
    let entries = std::fs::read_dir(&dir)
        .map_err(|err| IdeError::new(IdeErrorKind::Internal, format!("read_dir failed: {err}")))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with('.') {
            continue;
        }
        let next_relative = if relative.as_os_str().is_empty() {
            PathBuf::from(file_name.as_ref())
        } else {
            relative.join(file_name.as_ref())
        };
        if path.is_dir() {
            collect_workspace_files(root, &next_relative, out)?;
            continue;
        }
        out.push(next_relative.to_string_lossy().replace('\\', "/"));
    }
    Ok(())
}

fn collect_source_files(
    root: &Path,
    relative: &Path,
    out: &mut Vec<String>,
) -> Result<(), IdeError> {
    let mut files = Vec::new();
    collect_workspace_files(root, relative, &mut files)?;
    for path in files {
        if path.to_ascii_lowercase().ends_with(".st") {
            out.push(path);
        }
    }
    Ok(())
}

fn collect_workspace_tree(root: &Path, relative: &Path) -> Result<Vec<IdeTreeNode>, IdeError> {
    let dir = root.join(relative);
    let mut entries = std::fs::read_dir(&dir)
        .map_err(|err| IdeError::new(IdeErrorKind::Internal, format!("read_dir failed: {err}")))?
        .flatten()
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());

    let mut nodes = Vec::new();
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let rel = if relative.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            relative.join(&name)
        };
        if path.is_dir() {
            let children = collect_workspace_tree(root, &rel)?;
            nodes.push(IdeTreeNode {
                name: name.clone(),
                path: rel.to_string_lossy().replace('\\', "/"),
                kind: "directory".to_string(),
                children,
            });
            continue;
        }
        nodes.push(IdeTreeNode {
            name,
            path: rel.to_string_lossy().replace('\\', "/"),
            kind: "file".to_string(),
            children: Vec::new(),
        });
    }
    Ok(nodes)
}

fn prune_expired(state: &mut IdeStateInner, now: u64) {
    let expired = state
        .sessions
        .iter()
        .filter_map(|(token, session)| {
            if session.expires_at <= now {
                Some(token.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if expired.is_empty() {
        return;
    }

    for token in &expired {
        state.sessions.remove(token);
        state.frontend_telemetry_by_session.remove(token);
        state.analysis_cache.remove(token);
    }
    for doc in state.documents.values_mut() {
        for token in &expired {
            doc.opened_by.remove(token);
        }
    }
}

fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    fn project_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "trust-runtime-web-ide-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create project dir");
        path
    }

    fn write_source(project: &Path, rel: &str, content: &str) {
        let path = project.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create source parent");
        }
        std::fs::write(path, content).expect("write source");
    }

    #[test]
    fn auth_and_session_lifecycle_contract() {
        let project = project_dir("session");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let clock = Arc::new(AtomicU64::new(10_000));
        let state = WebIdeState::with_clock(
            Some(project.clone()),
            Arc::new({
                let clock = clock.clone();
                move || clock.load(Ordering::SeqCst)
            }),
        );

        let err = state.list_sources("missing").expect_err("missing session");
        assert_eq!(err.kind(), IdeErrorKind::Unauthorized);

        let session = state
            .create_session(IdeRole::Viewer)
            .expect("create viewer session");
        let files = state
            .list_sources(&session.token)
            .expect("list files with session");
        assert_eq!(files, vec!["main.st".to_string()]);

        clock.store(10_000 + SESSION_TTL_SECS + 1, Ordering::SeqCst);
        let expired_err = state
            .open_source(&session.token, "main.st")
            .expect_err("session should be expired");
        assert_eq!(expired_err.kind(), IdeErrorKind::Unauthorized);

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn session_activity_renews_ttl_and_idle_expiry_still_applies() {
        let project = project_dir("sliding-ttl");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let clock = Arc::new(AtomicU64::new(20_000));
        let state = WebIdeState::with_clock(
            Some(project.clone()),
            Arc::new({
                let clock = clock.clone();
                move || clock.load(Ordering::SeqCst)
            }),
        );

        let session = state
            .create_session(IdeRole::Viewer)
            .expect("create viewer session");

        clock.store(20_000 + (SESSION_TTL_SECS / 2), Ordering::SeqCst);
        let _ = state
            .list_sources(&session.token)
            .expect("active request should renew ttl");

        clock.store(20_000 + SESSION_TTL_SECS + 5, Ordering::SeqCst);
        let _ = state
            .open_source(&session.token, "main.st")
            .expect("session should still be valid after renewal");

        clock.store(20_000 + (2 * SESSION_TTL_SECS) + 10, Ordering::SeqCst);
        let expired = state
            .list_sources(&session.token)
            .expect_err("idle session should expire");
        assert_eq!(expired.kind(), IdeErrorKind::Unauthorized);

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn collaborative_conflict_detected_with_expected_version() {
        let project = project_dir("conflict");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let state = WebIdeState::new(Some(project.clone()));
        let s1 = state
            .create_session(IdeRole::Editor)
            .expect("create editor session 1");
        let s2 = state
            .create_session(IdeRole::Editor)
            .expect("create editor session 2");

        let doc1 = state
            .open_source(&s1.token, "main.st")
            .expect("open from s1");
        let doc2 = state
            .open_source(&s2.token, "main.st")
            .expect("open from s2");
        assert_eq!(doc1.version, doc2.version);

        let write1 = state
            .apply_source(
                &s1.token,
                "main.st",
                doc1.version,
                "PROGRAM Main\nVAR\nA : INT;\nEND_VAR\nEND_PROGRAM\n".to_string(),
                true,
            )
            .expect("apply first edit");
        assert!(write1.version > doc1.version);

        let conflict = state
            .apply_source(
                &s2.token,
                "main.st",
                doc2.version,
                "PROGRAM Main\nVAR\nB : INT;\nEND_VAR\nEND_PROGRAM\n".to_string(),
                true,
            )
            .expect_err("stale write must conflict");
        assert_eq!(conflict.kind(), IdeErrorKind::Conflict);
        assert_eq!(conflict.current_version(), Some(write1.version));

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn latency_and_resource_budgets_are_enforced() {
        let project = project_dir("budget");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let state = WebIdeState::new(Some(project.clone()));
        let session = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");
        let mut snapshot = state
            .open_source(&session.token, "main.st")
            .expect("open source");

        let runs = 80_u32;
        let mut total = Duration::ZERO;
        let mut max = Duration::ZERO;

        for idx in 0..runs {
            let started = Instant::now();
            let result = state
                .apply_source(
                    &session.token,
                    "main.st",
                    snapshot.version,
                    format!(
                        "PROGRAM Main\\nVAR\\nA : INT := {};\\nEND_VAR\\nEND_PROGRAM\\n",
                        idx
                    ),
                    true,
                )
                .expect("apply edit within budget");
            let elapsed = started.elapsed();
            total += elapsed;
            max = max.max(elapsed);
            snapshot.version = result.version;
        }

        let avg = total / runs;
        assert!(
            max < Duration::from_millis(250),
            "max apply latency {:?} exceeded budget",
            max
        );
        assert!(
            avg < Duration::from_millis(40),
            "avg apply latency {:?} exceeded budget",
            avg
        );

        let too_large = "X".repeat(MAX_FILE_BYTES + 1);
        let err = state
            .apply_source(&session.token, "main.st", snapshot.version, too_large, true)
            .expect_err("oversized payload should fail");
        assert_eq!(err.kind(), IdeErrorKind::TooLarge);

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn diagnostics_hover_and_completion_contracts_are_exposed() {
        let project = project_dir("analysis");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let state = WebIdeState::new(Some(project.clone()));
        let session = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");

        let diagnostics = state
            .diagnostics(
                &session.token,
                "main.st",
                Some(
                    "PROGRAM Main\nVAR\nCounter : INT;\nEND_VAR\n\nCounter := UnknownSymbol + 1;\nEND_PROGRAM\n"
                        .to_string(),
                ),
            )
            .expect("diagnostics");
        assert!(
            diagnostics
                .iter()
                .any(|item| item.message.contains("UnknownSymbol")),
            "expected unresolved symbol diagnostic"
        );

        let hover = state
            .hover(
                &session.token,
                "main.st",
                Some(
                    "PROGRAM Main\nVAR\nCounter : INT;\nEND_VAR\n\nCounter := Counter + 1;\nEND_PROGRAM\n"
                        .to_string(),
                ),
                Position {
                    line: 5,
                    character: 2,
                },
            )
            .expect("hover");
        assert!(hover.is_some(), "hover payload should be available");

        let completion = state
            .completion(
                &session.token,
                "main.st",
                Some("PRO\nPROGRAM Main\nEND_PROGRAM\n".to_string()),
                Position {
                    line: 0,
                    character: 3,
                },
                Some(20),
            )
            .expect("completion");
        assert!(
            completion.iter().any(|item| item.label == "PROGRAM"),
            "completion should include PROGRAM"
        );

        let in_scope_completion = state
            .completion(
                &session.token,
                "main.st",
                Some(
                    "PROGRAM Main\nVAR\nCounter : INT;\nEND_VAR\n\nCoun\nEND_PROGRAM\n".to_string(),
                ),
                Position {
                    line: 5,
                    character: 4,
                },
                Some(20),
            )
            .expect("in-scope completion");
        assert!(
            in_scope_completion
                .iter()
                .take(3)
                .any(|item| item.label == "Counter"),
            "expected in-scope symbol in top-3 suggestions"
        );

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn format_structured_text_document_indents_common_blocks() {
        let input = "PROGRAM Main\nVAR\nCounter:INT;\nEND_VAR\nIF Counter > 0 THEN\nCounter:=Counter+1;\nELSE\nCounter:=0;\nEND_IF\nEND_PROGRAM\n";
        let expected = "PROGRAM Main\n  VAR\n    Counter:INT;\n  END_VAR\n  IF Counter > 0 THEN\n    Counter:=Counter+1;\n  ELSE\n    Counter:=0;\n  END_IF\nEND_PROGRAM\n";
        assert_eq!(format_structured_text_document(input), expected);
    }

    #[test]
    fn format_source_endpoint_returns_formatted_content_without_write() {
        let project = project_dir("format-source");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let state = WebIdeState::new(Some(project.clone()));
        let session = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");

        let result = state
            .format_source(
                &session.token,
                "main.st",
                Some("PROGRAM Main\nVAR\nA:INT;\nEND_VAR\nEND_PROGRAM\n".to_string()),
            )
            .expect("format source");
        assert_eq!(result.path, "main.st");
        assert!(result.changed);
        assert!(result.content.contains("  VAR"));
        assert!(result.content.contains("    A:INT;"));

        let disk = std::fs::read_to_string(project.join("main.st")).expect("read disk source");
        assert_eq!(disk, "PROGRAM Main\nEND_PROGRAM\n");

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn health_snapshot_reports_active_state() {
        let project = project_dir("health");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let state = WebIdeState::new(Some(project.clone()));
        let viewer = state
            .create_session(IdeRole::Viewer)
            .expect("create viewer session");
        let editor = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");
        let _ = state
            .open_source(&editor.token, "main.st")
            .expect("open source");

        let health = state.health(&viewer.token).expect("health");
        assert_eq!(health.active_sessions, 2);
        assert_eq!(health.editor_sessions, 1);
        assert_eq!(health.tracked_documents, 1);
        assert_eq!(health.open_document_handles, 1);
        assert_eq!(health.frontend_telemetry.bootstrap_failures, 0);
        assert_eq!(health.frontend_telemetry.analysis_timeouts, 0);

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn frontend_telemetry_is_aggregated_in_health_snapshot() {
        let project = project_dir("frontend-telemetry");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let state = WebIdeState::new(Some(project.clone()));
        let s1 = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");
        let s2 = state
            .create_session(IdeRole::Viewer)
            .expect("create viewer session");

        state
            .record_frontend_telemetry(
                &s1.token,
                WebIdeFrontendTelemetry {
                    bootstrap_failures: 1,
                    analysis_timeouts: 2,
                    worker_restarts: 0,
                    autosave_failures: 3,
                },
            )
            .expect("record telemetry session 1");
        state
            .record_frontend_telemetry(
                &s2.token,
                WebIdeFrontendTelemetry {
                    bootstrap_failures: 4,
                    analysis_timeouts: 1,
                    worker_restarts: 2,
                    autosave_failures: 0,
                },
            )
            .expect("record telemetry session 2");

        let health = state.health(&s1.token).expect("health");
        assert_eq!(health.frontend_telemetry.bootstrap_failures, 5);
        assert_eq!(health.frontend_telemetry.analysis_timeouts, 3);
        assert_eq!(health.frontend_telemetry.worker_restarts, 2);
        assert_eq!(health.frontend_telemetry.autosave_failures, 3);

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn workspace_search_respects_include_and_exclude_globs() {
        let project = project_dir("workspace-search-globs");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");
        write_source(
            &project,
            "types.st",
            "TYPE\nMyType : STRUCT\nvalue : INT;\nEND_STRUCT;\nEND_TYPE\n",
        );

        let state = WebIdeState::new(Some(project.clone()));
        let session = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");

        let scoped = state
            .workspace_search(&session.token, "MyType", Some("types.st"), None, 50)
            .expect("search with include glob");
        assert!(!scoped.is_empty());
        assert!(scoped.iter().all(|hit| hit.path == "types.st"));

        let excluded = state
            .workspace_search(&session.token, "MyType", None, Some("types.st"), 50)
            .expect("search with exclude glob");
        assert!(excluded.iter().all(|hit| hit.path != "types.st"));

        let invalid = state
            .workspace_search(&session.token, "Main", Some("["), None, 10)
            .expect_err("invalid include glob should fail");
        assert_eq!(invalid.kind(), IdeErrorKind::InvalidInput);

        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn project_selection_and_switch_flow_updates_active_root() {
        let project_a = project_dir("project-switch-a");
        let project_b = project_dir("project-switch-b");
        write_source(&project_a, "main.st", "PROGRAM Main\nEND_PROGRAM\n");
        write_source(&project_b, "alt.st", "PROGRAM Alt\nEND_PROGRAM\n");

        let state = WebIdeState::new(None);
        let session = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");

        let initial = state
            .project_selection(&session.token)
            .expect("project selection");
        assert!(initial.active_project.is_none());

        let switched = state
            .set_active_project(&session.token, project_b.to_string_lossy().as_ref())
            .expect("set active project");
        assert!(switched
            .active_project
            .as_ref()
            .is_some_and(|path| path.contains("project-switch-b")));

        let files = state
            .list_sources(&session.token)
            .expect("list switched project files");
        assert_eq!(files, vec!["alt.st".to_string()]);

        let _ = std::fs::remove_dir_all(project_a);
        let _ = std::fs::remove_dir_all(project_b);
    }

    #[test]
    fn fs_audit_log_tracks_mutating_operations() {
        let project = project_dir("fs-audit");
        write_source(&project, "main.st", "PROGRAM Main\nEND_PROGRAM\n");

        let state = WebIdeState::new(Some(project.clone()));
        let session = state
            .create_session(IdeRole::Editor)
            .expect("create editor session");

        let _ = state
            .create_entry(&session.token, "folder_a", true, None, true)
            .expect("create directory");
        let _ = state
            .create_entry(
                &session.token,
                "folder_a/extra.st",
                false,
                Some("PROGRAM Extra\nEND_PROGRAM\n".to_string()),
                true,
            )
            .expect("create file");
        let _ = state
            .rename_entry(
                &session.token,
                "folder_a/extra.st",
                "folder_a/renamed_extra.st",
                true,
            )
            .expect("rename file");
        let _ = state
            .delete_entry(&session.token, "folder_a/renamed_extra.st", true)
            .expect("delete file");

        let audit = state
            .fs_audit(&session.token, 20)
            .expect("read fs audit events");
        assert!(audit.len() >= 4);
        let health = state.health(&session.token).expect("health");
        assert!(health.fs_mutation_events >= 4);

        let _ = std::fs::remove_dir_all(project);
    }
}
