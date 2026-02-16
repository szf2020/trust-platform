// truST Web IDE – application logic

// ── Constants & Configuration ──────────────────────────

const DRAFT_PREFIX = "trust.ide.draft.";
const THEME_STORAGE_KEY = "trustTheme";
const IDE_LEFT_WIDTH_KEY = "trust.ide.leftWidth";
const IDE_RIGHT_WIDTH_KEY = "trust.ide.rightWidth";
const A11Y_REPORT_LINK = "docs/guides/WEB_IDE_ACCESSIBILITY_BASELINE.md";
const IDE_PRESENCE_CHANNEL = "trust.ide.presence";
const IDE_PRESENCE_STORAGE_KEY = "trust.ide.presence.event";
const IDE_PRESENCE_CLAIM_TTL_MS = 12_000;
const API_DEFAULT_TIMEOUT_MS = 6_000;
const ANALYSIS_TIMEOUT_MS = 3_000;
const SESSION_EXPIRED_TEXT = "invalid or expired session";
const ST_LANGUAGE_ID = "trust-st";
const MONACO_MARKER_OWNER = "trust.ide";
const TAB_ID = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
const RECENT_PROJECTS_KEY = "trust.ide.recentProjects";
const MAX_RECENT_PROJECTS = 5;

// ── State ──────────────────────────────────────────────

let monaco;
let ensureStyleInjected = () => {};
let completionProviderDisposable = null;
let hoverProviderDisposable = null;
let startCompletion = () => {};
let cursorInsightTimer = null;
let completionTriggerTimer = null;
let cursorHoverPopupTimer = null;
let documentHighlightDecorations = [];
let documentHighlightTimer = null;
let wasmClient = null;

const state = {
  tabId: TAB_ID,
  online: navigator.onLine,
  ready: false,
  sessionToken: null,
  writeEnabled: false,
  files: [],
  tree: [],
  activeProject: null,
  startupProject: null,
  fileFilter: "",
  selectedPath: null,
  expandedDirs: new Set([""]),
  openTabs: new Map(),
  activePath: null,
  editorView: null,
  secondaryEditorView: null,
  secondaryPath: null,
  secondaryOpenTabs: new Set(),
  splitEnabled: false,
  activePane: "primary",
  diagnostics: [],
  references: [],
  searchHits: [],
  latencySamples: [],
  diagnosticsTimer: null,
  diagnosticsTicket: 0,
  autosaveTimer: null,
  healthTimer: null,
  telemetryTimer: null,
  taskPollTimer: null,
  suppressEditorChange: false,
  editorDisposables: [],
  activeTaskId: null,
  lastFailedAction: null,
  presenceChannel: null,
  peerClaims: new Map(),
  collisionPath: null,
  analysis: {
    degraded: false,
    consecutiveFailures: 0,
    lastNoticeAtMs: 0,
  },
  telemetry: {
    bootstrap_failures: 0,
    analysis_timeouts: 0,
    worker_restarts: 0,
    autosave_failures: 0,
  },
  commandFilter: "",
  commands: [],
  selectedCommandIndex: 0,
  contextPath: null,
  browseVisible: false,
};

// ── DOM References ─────────────────────────────────────

const el = {
  fileTree: document.getElementById("fileTree"),
  fileFilterInput: document.getElementById("fileFilterInput"),
  newFileBtn: document.getElementById("newFileBtn"),
  newFolderBtn: document.getElementById("newFolderBtn"),
  renamePathBtn: document.getElementById("renamePathBtn"),
  deletePathBtn: document.getElementById("deletePathBtn"),
  breadcrumbBar: document.getElementById("breadcrumbBar"),
  sidebarResizeHandle: document.getElementById("sidebarResizeHandle"),
  tabBar: document.getElementById("tabBar"),
  ideTitle: document.getElementById("ideTitle"),
  scopeNote: document.getElementById("scopeNote"),
  statusMode: document.getElementById("statusMode"),
  statusProject: document.getElementById("statusProject"),
  connectionBadge: document.getElementById("connectionBadge"),
  saveBadge: document.getElementById("saveBadge"),
  statusText: document.getElementById("statusText"),
  draftInfo: document.getElementById("draftInfo"),
  editorTitle: document.getElementById("editorTitle"),
  cursorLabel: document.getElementById("cursorLabel"),
  problemsPanel: document.getElementById("problemsPanel"),
  referencesPanel: document.getElementById("referencesPanel"),
  searchPanel: document.getElementById("searchPanel"),
  taskStatus: document.getElementById("taskStatus"),
  retryActionBtn: document.getElementById("retryActionBtn"),
  taskOutput: document.getElementById("taskOutput"),
  taskLinksPanel: document.getElementById("taskLinksPanel"),
  healthPanel: document.getElementById("healthPanel"),
  latencyBadge: document.getElementById("latencyBadge"),
  editorPanePrimary: document.getElementById("editorPanePrimary"),
  editorPaneSecondary: document.getElementById("editorPaneSecondary"),
  editorMount: document.getElementById("editorMount"),
  editorMountSecondary: document.getElementById("editorMountSecondary"),
  tabBarPrimary: document.getElementById("tabBarPrimary"),
  tabBarSecondary: document.getElementById("tabBarSecondary"),
  insightResizeHandle: document.getElementById("insightResizeHandle"),
  editorWelcome: document.getElementById("editorWelcome"),
  welcomeOpenBtn: document.getElementById("welcomeOpenBtn"),
  welcomeQuickOpenBtn: document.getElementById("welcomeQuickOpenBtn"),
  editorGrid: document.getElementById("editorGrid"),
  saveBtn: document.getElementById("saveBtn"),
  saveAllBtn: document.getElementById("saveAllBtn"),
  validateBtn: document.getElementById("validateBtn"),
  buildBtn: document.getElementById("buildBtn"),
  testBtn: document.getElementById("testBtn"),
  splitBtn: document.getElementById("splitBtn"),
  openProjectBtn: document.getElementById("openProjectBtn"),
  quickOpenBtn: document.getElementById("quickOpenBtn"),
  themeToggle: document.getElementById("themeToggle"),
  commandPalette: document.getElementById("commandPalette"),
  commandInput: document.getElementById("commandInput"),
  commandList: document.getElementById("commandList"),
  cmdPaletteBtn: document.getElementById("cmdPaletteBtn"),
  treeContextMenu: document.getElementById("treeContextMenu"),
  ctxOpenBtn: document.getElementById("ctxOpenBtn"),
  ctxNewFileBtn: document.getElementById("ctxNewFileBtn"),
  ctxNewFolderBtn: document.getElementById("ctxNewFolderBtn"),
  ctxRenameBtn: document.getElementById("ctxRenameBtn"),
  ctxDeleteBtn: document.getElementById("ctxDeleteBtn"),
  inputModal: document.getElementById("inputModal"),
  inputModalTitle: document.getElementById("inputModalTitle"),
  inputModalField: document.getElementById("inputModalField"),
  inputModalOk: document.getElementById("inputModalOk"),
  inputModalCancel: document.getElementById("inputModalCancel"),
  confirmModal: document.getElementById("confirmModal"),
  confirmModalTitle: document.getElementById("confirmModalTitle"),
  confirmModalMessage: document.getElementById("confirmModalMessage"),
  confirmModalOk: document.getElementById("confirmModalOk"),
  confirmModalCancel: document.getElementById("confirmModalCancel"),
  openProjectPanel: document.getElementById("openProjectPanel"),
  openProjectInput: document.getElementById("openProjectInput"),
  openProjectRecent: document.getElementById("openProjectRecent"),
  openProjectOk: document.getElementById("openProjectOk"),
  openProjectCancel: document.getElementById("openProjectCancel"),
  browseBtn: document.getElementById("browseBtn"),
  browseListing: document.getElementById("browseListing"),
  browseBreadcrumbs: document.getElementById("browseBreadcrumbs"),
  browseEntries: document.getElementById("browseEntries"),
};

// ── Utilities ──────────────────────────────────────────

function nowLabel() {
  return new Date().toLocaleTimeString();
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function isStructuredTextPath(path) {
  return String(path || "").toLowerCase().endsWith(".st");
}

function formatTimestampMs(value) {
  const asNumber = Number(value || 0);
  if (!Number.isFinite(asNumber) || asNumber <= 0) {
    return "--";
  }
  return new Date(asNumber).toLocaleTimeString();
}

function setStatus(text) {
  el.statusText.textContent = text;
}

function bumpTelemetry(key, amount = 1) {
  const current = Number(state.telemetry[key] || 0);
  state.telemetry[key] = current + amount;
}

function isTimeoutMessage(message) {
  const text = String(message || "").toLowerCase();
  return text.includes("timeout");
}

function bindAction(element, action, errorLabel) {
  element.addEventListener("click", () => {
    action().catch((error) => {
      if (errorLabel) setStatus(`${errorLabel}: ${error.message || error}`);
    });
  });
}

// ── API Layer ──────────────────────────────────────────

function apiHeaders(extra = {}, includeSession = true) {
  const headers = {
    "Content-Type": "application/json",
    ...extra,
  };
  if (includeSession && state.sessionToken) {
    headers["X-Trust-Ide-Session"] = state.sessionToken;
  }
  return headers;
}

async function requestNewSession() {
  const role = state.writeEnabled ? "editor" : "viewer";
  const response = await fetch("/api/ide/session", {
    method: "POST",
    headers: apiHeaders({}, false),
    body: JSON.stringify({role}),
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : {};
  if (!response.ok || payload.ok === false) {
    const message = payload.error || `session refresh failed (${response.status})`;
    throw new Error(message);
  }
  state.sessionToken = payload.result?.token || null;
  return payload.result;
}

async function apiJson(url, options = {}) {
  const {
    timeoutMs = API_DEFAULT_TIMEOUT_MS,
    allowSessionRetry = true,
    ...fetchOptions
  } = options;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const opts = {
    method: "GET",
    ...fetchOptions,
    headers: {
      ...(fetchOptions.headers || {}),
    },
    signal: controller.signal,
  };

  try {
    const response = await fetch(url, opts);
    const text = await response.text();
    let payload = {};
    try {
      payload = text ? JSON.parse(text) : {};
    } catch {
      payload = {ok: false, error: text || "invalid response"};
    }
    if (!response.ok || payload.ok === false) {
      const message = payload.error || `request failed (${response.status})`;
      if (
        allowSessionRetry &&
        state.ready &&
        String(message).toLowerCase().includes(SESSION_EXPIRED_TEXT)
      ) {
        await requestNewSession();
        return await apiJson(url, {
          ...options,
          allowSessionRetry: false,
        });
      }
      throw new Error(message);
    }
    state.online = true;
    updateConnectionBadge();
    return payload.result;
  } catch (error) {
    if (error?.name === "AbortError") {
      throw new Error(`request timeout after ${timeoutMs}ms`);
    }
    if (error instanceof TypeError) {
      state.online = false;
      updateConnectionBadge();
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

// ── Theme & Layout ─────────────────────────────────────

function applyTheme(theme) {
  const next = theme || "light";
  document.body.dataset.theme = next;
  localStorage.setItem(THEME_STORAGE_KEY, next);
  el.themeToggle.textContent = next === "dark" ? "Light mode" : "Dark mode";
  if (monaco) {
    monaco.editor.setTheme(next === "dark" ? "trust-dark" : "trust-light");
  }
}

function toggleTheme() {
  const active = document.body.dataset.theme === "dark" ? "dark" : "light";
  applyTheme(active === "dark" ? "light" : "dark");
}

function applyWorkbenchSizing() {
  const left = Number(localStorage.getItem(IDE_LEFT_WIDTH_KEY) || 290);
  const right = Number(localStorage.getItem(IDE_RIGHT_WIDTH_KEY) || 320);
  document.documentElement.style.setProperty("--ide-left-width", `${clamp(left, 220, 520)}px`);
  document.documentElement.style.setProperty("--ide-right-width", `${clamp(right, 250, 520)}px`);
}

function bindResizeHandles() {
  const startDrag = (kind, event) => {
    if (window.matchMedia && window.matchMedia("(max-width: 1160px)").matches) {
      return;
    }
    event.preventDefault();
    const handle = kind === "left" ? el.sidebarResizeHandle : el.insightResizeHandle;
    handle.classList.add("dragging");

    const onMove = (moveEvent) => {
      if (kind === "left") {
        const width = clamp(moveEvent.clientX, 220, 520);
        document.documentElement.style.setProperty("--ide-left-width", `${width}px`);
        localStorage.setItem(IDE_LEFT_WIDTH_KEY, String(width));
        return;
      }
      const shellRect = document.querySelector(".ide-shell")?.getBoundingClientRect();
      if (!shellRect) {
        return;
      }
      const width = clamp(shellRect.right - moveEvent.clientX, 250, 520);
      document.documentElement.style.setProperty("--ide-right-width", `${width}px`);
      localStorage.setItem(IDE_RIGHT_WIDTH_KEY, String(width));
    };

    const onUp = () => {
      handle.classList.remove("dragging");
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  el.sidebarResizeHandle.addEventListener("mousedown", (event) => startDrag("left", event));
  el.insightResizeHandle.addEventListener("mousedown", (event) => startDrag("right", event));
}

function updateConnectionBadge() {
  if (state.online) {
    el.connectionBadge.className = "ide-badge ok";
    el.connectionBadge.textContent = "online";
  } else {
    el.connectionBadge.className = "ide-badge err";
    el.connectionBadge.textContent = "offline";
  }
}

function updateSaveBadge(kind, text) {
  if (kind === "ok") {
    el.saveBadge.className = "ide-badge ok";
  } else if (kind === "warn") {
    el.saveBadge.className = "ide-badge warn";
  } else if (kind === "err") {
    el.saveBadge.className = "ide-badge err";
  } else {
    el.saveBadge.className = "ide-badge";
  }
  el.saveBadge.textContent = text;
}

function updateLatencyBadge() {
  if (state.analysis.degraded) {
    el.latencyBadge.className = "ide-badge warn";
    el.latencyBadge.textContent = "analysis degraded";
    return;
  }
  if (state.latencySamples.length === 0) {
    el.latencyBadge.textContent = "latency --";
    return;
  }
  const sorted = [...state.latencySamples].sort((a, b) => a - b);
  const p95Index = Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95));
  const p95 = sorted[p95Index];
  el.latencyBadge.textContent = `diag p95 ${Math.round(p95)}ms`;
  if (p95 > 280) {
    el.latencyBadge.className = "ide-badge warn";
  } else {
    el.latencyBadge.className = "ide-badge ok";
  }
}

function updateAnalysisModeBadge() {
  if (state.analysis.degraded) {
    el.latencyBadge.className = "ide-badge warn";
    el.latencyBadge.textContent = "analysis degraded";
    return;
  }
  updateLatencyBadge();
}

// ── Monaco / Editor ────────────────────────────────────

function monacoLanguageForPath(path) {
  const normalized = String(path || "").toLowerCase();
  if (normalized.endsWith(".st")) return ST_LANGUAGE_ID;
  if (normalized.endsWith(".json")) return "json";
  if (normalized.endsWith(".toml")) return "ini";
  if (normalized.endsWith(".md")) return "markdown";
  if (normalized.endsWith(".yaml") || normalized.endsWith(".yml")) return "yaml";
  if (normalized.endsWith(".xml")) return "xml";
  if (normalized.endsWith(".js")) return "javascript";
  if (normalized.endsWith(".ts")) return "typescript";
  if (normalized.endsWith(".css")) return "css";
  if (normalized.endsWith(".html")) return "html";
  return "plaintext";
}

function activeModel() {
  return state.editorView ? state.editorView.getModel() : null;
}

function fromMonacoPosition(position) {
  if (!position) {
    return {line: 0, character: 0};
  }
  return {
    line: Math.max(0, Number(position.lineNumber || 1) - 1),
    character: Math.max(0, Number(position.column || 1) - 1),
  };
}

function toMonacoPosition(position, model) {
  const safeModel = model || activeModel();
  const maxLines = safeModel ? safeModel.getLineCount() : 1;
  const line = clamp(Number(position?.line ?? 0) + 1, 1, Math.max(1, maxLines));
  const maxColumn = safeModel ? safeModel.getLineMaxColumn(line) : 1;
  const column = clamp(Number(position?.character ?? 0) + 1, 1, Math.max(1, maxColumn));
  return new monaco.Position(line, column);
}

function toMonacoRange(range, model) {
  const safeModel = model || activeModel();
  const start = toMonacoPosition(range?.start || {line: 0, character: 0}, safeModel);
  const end = toMonacoPosition(range?.end || range?.start || {line: 0, character: 1}, safeModel);
  return new monaco.Range(
    start.lineNumber,
    start.column,
    Math.max(start.lineNumber, end.lineNumber),
    end.lineNumber < start.lineNumber ? start.column : Math.max(start.column, end.column),
  );
}

function positionToContentOffset(content, position) {
  const targetLine = Number(position?.line ?? 0);
  const targetChar = Number(position?.character ?? 0);
  let line = 0;
  let character = 0;
  for (let i = 0; i < content.length; i++) {
    if (line === targetLine && character === targetChar) {
      return i;
    }
    if (content[i] === "\n") {
      if (line === targetLine) {
        return i;
      }
      line++;
      character = 0;
    } else {
      character++;
    }
  }
  if (line === targetLine) {
    return content.length;
  }
  return null;
}

function monacoCompletionKind(kind) {
  const value = String(kind || "").toLowerCase();
  if (value.includes("function")) return monaco.languages.CompletionItemKind.Function;
  if (value.includes("method")) return monaco.languages.CompletionItemKind.Method;
  if (value.includes("class")) return monaco.languages.CompletionItemKind.Class;
  if (value.includes("module")) return monaco.languages.CompletionItemKind.Module;
  if (value.includes("field")) return monaco.languages.CompletionItemKind.Field;
  if (value.includes("property")) return monaco.languages.CompletionItemKind.Property;
  if (value.includes("variable")) return monaco.languages.CompletionItemKind.Variable;
  if (value.includes("enum")) return monaco.languages.CompletionItemKind.Enum;
  if (value.includes("keyword")) return monaco.languages.CompletionItemKind.Keyword;
  if (value.includes("snippet")) return monaco.languages.CompletionItemKind.Snippet;
  if (value.includes("type")) return monaco.languages.CompletionItemKind.TypeParameter;
  return monaco.languages.CompletionItemKind.Text;
}

function monacoMarkerSeverity(severity) {
  const value = String(severity || "").toLowerCase();
  if (value.includes("error")) return monaco.MarkerSeverity.Error;
  if (value.includes("info")) return monaco.MarkerSeverity.Info;
  if (value.includes("hint")) return monaco.MarkerSeverity.Hint;
  return monaco.MarkerSeverity.Warning;
}

function extractLocalCompletionCandidates(model) {
  if (!model) {
    return [];
  }
  const text = model.getValue();
  const identifiers = new Set();
  const matches = text.matchAll(/[A-Za-z_][A-Za-z0-9_]*/g);
  for (const match of matches) {
    if (match && match[0]) {
      identifiers.add(match[0]);
    }
  }
  const stKeywords = [
    "PROGRAM", "END_PROGRAM", "FUNCTION", "END_FUNCTION", "FUNCTION_BLOCK",
    "END_FUNCTION_BLOCK", "VAR", "END_VAR", "VAR_INPUT", "VAR_OUTPUT",
    "VAR_IN_OUT", "VAR_GLOBAL", "IF", "THEN", "ELSE", "ELSIF", "END_IF",
    "CASE", "OF", "END_CASE", "FOR", "TO", "BY", "DO", "END_FOR",
    "WHILE", "END_WHILE", "REPEAT", "UNTIL", "END_REPEAT", "TRUE", "FALSE",
    "BOOL", "INT", "DINT", "UINT", "UDINT", "REAL", "LREAL", "STRING",
  ];
  for (const keyword of stKeywords) {
    identifiers.add(keyword);
  }
  return Array.from(identifiers).sort((a, b) => a.localeCompare(b));
}

function fallbackCompletionRange(model, position) {
  const word = model.getWordUntilPosition(position);
  return new monaco.Range(
    position.lineNumber,
    word.startColumn || position.column,
    position.lineNumber,
    word.endColumn || position.column,
  );
}

function buildLocalCompletionSuggestions(model, position, limit = 120) {
  const range = fallbackCompletionRange(model, position);
  return extractLocalCompletionCandidates(model)
    .slice(0, limit)
    .map((label) => ({
      label,
      kind: /^[A-Z_]+$/.test(label)
        ? monaco.languages.CompletionItemKind.Keyword
        : monaco.languages.CompletionItemKind.Variable,
      detail: "local symbol",
      insertText: label,
      range,
    }));
}

function normalizeHoverContentValue(contents) {
  if (typeof contents === "string") {
    return contents.trim();
  }
  if (Array.isArray(contents)) {
    const parts = contents
      .map((entry) => {
        if (typeof entry === "string") {
          return entry.trim();
        }
        if (entry && typeof entry.value === "string") {
          return entry.value.trim();
        }
        return "";
      })
      .filter((value) => value.length > 0);
    return parts.join("\n\n").trim();
  }
  if (contents && typeof contents === "object" && typeof contents.value === "string") {
    return contents.value.trim();
  }
  return "";
}

function buildFallbackHover(model, position) {
  const word = model.getWordAtPosition(position);
  if (!word || !word.word) {
    return null;
  }
  return {
    range: new monaco.Range(
      position.lineNumber,
      word.startColumn,
      position.lineNumber,
      word.endColumn,
    ),
    contents: [{value: `\`\`\`st\n${word.word}\n\`\`\``}],
  };
}

function defineMonacoThemes() {
  monaco.editor.defineTheme("trust-light", {
    base: "vs",
    inherit: true,
    rules: [
      {token: "keyword.st", foreground: "0f766e", fontStyle: "bold"},
      {token: "number.st", foreground: "875f00"},
    ],
    colors: {
      "editor.background": "#ffffff",
      "editorCursor.foreground": "#0f766e",
      "editorLineNumber.foreground": "#7e8aa1",
      "editorLineNumber.activeForeground": "#213047",
      "editorGutter.background": "#f6f3ee",
      "editor.selectionBackground": "#0f766e22",
      "editor.inactiveSelectionBackground": "#0f766e11",
      "editor.wordHighlightBackground": "#0f766e30",
      "editor.wordHighlightStrongBackground": "#0f766e45",
      "editor.selectionHighlightBackground": "#0f766e20",
      "editor.selectionHighlightBorder": "#0f766e50",
      "editorWidget.background": "#f4f2ef",
      "editorWidget.foreground": "#1b1a18",
      "editorWidget.border": "#c8d8d4",
      "editorHoverWidget.background": "#f4f2ef",
      "editorHoverWidget.foreground": "#1b1a18",
      "editorHoverWidget.border": "#c8d8d4",
      "editorSuggestWidget.background": "#f4f2ef",
      "editorSuggestWidget.foreground": "#1b1a18",
      "editorSuggestWidget.border": "#c8d8d4",
      "editorSuggestWidget.selectedBackground": "#d9ece8",
      "editorSuggestWidget.highlightForeground": "#0f766e",
    },
  });
  monaco.editor.defineTheme("trust-dark", {
    base: "vs-dark",
    inherit: true,
    rules: [
      {token: "keyword.st", foreground: "14b8a6", fontStyle: "bold"},
      {token: "number.st", foreground: "e0c95a"},
    ],
    colors: {
      "editor.background": "#0f1115",
      "editorCursor.foreground": "#14b8a6",
      "editorLineNumber.foreground": "#6f7d9b",
      "editorLineNumber.activeForeground": "#dce6ff",
      "editorGutter.background": "#141821",
      "editor.selectionBackground": "#14b8a633",
      "editor.inactiveSelectionBackground": "#14b8a619",
      "editor.wordHighlightBackground": "#14b8a635",
      "editor.wordHighlightStrongBackground": "#14b8a650",
      "editor.selectionHighlightBackground": "#14b8a625",
      "editor.selectionHighlightBorder": "#14b8a655",
      "editorWidget.background": "#1f2430",
      "editorWidget.foreground": "#f2f2f2",
      "editorWidget.border": "#3c4b66",
      "editorHoverWidget.background": "#1f2430",
      "editorHoverWidget.foreground": "#f2f2f2",
      "editorHoverWidget.border": "#3c4b66",
      "editorSuggestWidget.background": "#1f2430",
      "editorSuggestWidget.foreground": "#f2f2f2",
      "editorSuggestWidget.border": "#3c4b66",
      "editorSuggestWidget.selectedBackground": "#1f3c4a",
      "editorSuggestWidget.highlightForeground": "#5eead4",
    },
  });
}

function configureMonacoLanguageSupport() {
  if (!monaco) {
    return;
  }

  if (!monaco.languages.getLanguages().some((language) => language.id === ST_LANGUAGE_ID)) {
    monaco.languages.register({
      id: ST_LANGUAGE_ID,
      extensions: [".st"],
      aliases: ["Structured Text", "ST"],
    });
    monaco.languages.setMonarchTokensProvider(ST_LANGUAGE_ID, {
      defaultToken: "",
      keywords: [
        "PROGRAM", "END_PROGRAM", "FUNCTION", "END_FUNCTION", "FUNCTION_BLOCK",
        "END_FUNCTION_BLOCK", "CONFIGURATION", "END_CONFIGURATION", "TASK", "INTERVAL",
        "PRIORITY", "PROGRAM", "WITH", "VAR", "VAR_INPUT", "VAR_OUTPUT", "VAR_IN_OUT",
        "VAR_GLOBAL", "VAR_CONFIG", "VAR_ACCESS", "END_VAR", "IF", "THEN", "ELSIF",
        "ELSE", "END_IF", "CASE", "OF", "END_CASE", "FOR", "TO", "BY", "DO", "END_FOR",
        "WHILE", "END_WHILE", "REPEAT", "UNTIL", "END_REPEAT", "TRUE", "FALSE", "BOOL",
        "INT", "DINT", "UINT", "UDINT", "REAL", "LREAL", "STRING",
      ],
      operators: [":=", "=", "<>", "<=", ">=", "<", ">", "+", "-", "*", "/", "AND", "OR", "NOT"],
      tokenizer: {
        root: [
          [/[A-Za-z_][A-Za-z0-9_]*/, {
            cases: {
              "@keywords": "keyword.st",
              "@default": "identifier",
            },
          }],
          [/[0-9]+(\.[0-9]+)?/, "number.st"],
          [/\/\/.*$/, "comment"],
          [/\(\*[\s\S]*?\*\)/, "comment"],
          [/".*?"/, "string"],
          [/'[^']*'/, "string"],
          [/[+\-*\/=<>:]+/, "operator"],
        ],
      },
    });
    monaco.languages.setLanguageConfiguration(ST_LANGUAGE_ID, {
      comments: {
        lineComment: "//",
        blockComment: ["(*", "*)"],
      },
      brackets: [
        ["(", ")"],
        ["[", "]"],
      ],
    });
  }

  defineMonacoThemes();

  completionProviderDisposable?.dispose();
  hoverProviderDisposable?.dispose();

  const triggerCharacters = [
    "_", ".", ...Array.from("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"),
  ];

  completionProviderDisposable = monaco.languages.registerCompletionItemProvider(ST_LANGUAGE_ID, {
    triggerCharacters,
    async provideCompletionItems(model, position) {
      if (!state.editorView || model !== state.editorView.getModel()) {
        return {suggestions: []};
      }
      const tab = activeTab();
      if (!tab || !isStructuredTextPath(tab.path)) {
        return {suggestions: []};
      }
      const cursor = fromMonacoPosition(position);
      const localSuggestions = buildLocalCompletionSuggestions(model, position);
      try {
        const items = await fetchCompletion(cursor, 80);
        if (!Array.isArray(items) || items.length === 0) {
          return {suggestions: localSuggestions};
        }
        const fallbackRange = fallbackCompletionRange(model, position);
        const suggestions = items
          .filter((item) => item && typeof item.label === "string" && item.label.length > 0)
          .map((item) => {
            let range = fallbackRange;
            if (item.text_edit?.range) {
              const candidateRange = toMonacoRange(item.text_edit.range, model);
              if (candidateRange.containsPosition(position)) {
                range = candidateRange;
              }
            }
            const priority = Number(item.sort_priority);
            const sortText = item.sort_text || (Number.isFinite(priority) ? String(priority).padStart(6, "0") : undefined);
            return {
              label: item.label,
              kind: monacoCompletionKind(item.kind),
              detail: item.detail || "",
              documentation: item.documentation ? {value: String(item.documentation)} : undefined,
              insertText: item.text_edit?.new_text || item.insert_text || item.label,
              range,
              sortText,
              filterText: item.filter_text || undefined,
            };
          });
        if (suggestions.length === 0) {
          return {suggestions: localSuggestions};
        }
        return {suggestions};
      } catch (error) {
        console.warn("[ide] completion failed:", error);
        return {suggestions: localSuggestions};
      }
    },
  });

  hoverProviderDisposable = monaco.languages.registerHoverProvider(ST_LANGUAGE_ID, {
    async provideHover(model, position) {
      if (!state.editorView || model !== state.editorView.getModel()) {
        return null;
      }
      const tab = activeTab();
      if (!tab || !isStructuredTextPath(tab.path)) {
        return null;
      }
      try {
        const response = await fetchHover(fromMonacoPosition(position));
        if (!response || !response.contents) {
          return buildFallbackHover(model, position);
        }
        const hoverText = normalizeHoverContentValue(response.contents);
        if (!hoverText) {
          return buildFallbackHover(model, position);
        }
        const hover = {
          contents: [{value: hoverText}],
        };
        if (response.range) {
          hover.range = toMonacoRange(response.range, model);
        }
        return hover;
      } catch (err) {
        console.warn("[ide] hover failed:", err);
        return buildFallbackHover(model, position);
      }
    },
  });

}

async function loadEditorModules() {
  try {
    const module = await import("/ide/assets/ide-monaco.20260215.js");
    ({monaco} = module);
    ({ensureStyleInjected} = module);
    ensureStyleInjected();
    configureMonacoLanguageSupport();
    startCompletion = () => {
      if (!state.editorView) {
        return;
      }
      state.editorView.focus();
      state.editorView.trigger("keyboard", "editor.action.triggerSuggest", {});
    };
    return true;
  } catch (error) {
    const message = String(error?.message || error);
    setStatus(`Editor modules failed to load: ${message}`);
    setStatus("Monaco assets could not be loaded from /ide/assets. Rebuild frontend bundle and refresh.");
    updateSaveBadge("err", "assets");
    return false;
  }
}

async function initWasmAnalysis() {
  try {
    const { TrustWasmAnalysisClient } = await import("/ide/wasm/analysis-client.js");
    wasmClient = new TrustWasmAnalysisClient({
      workerUrl: "/ide/wasm/worker.js",
      defaultTimeoutMs: 2000,
    });
    wasmClient.onStatus((status) => {
      console.log("[IDE] WASM status:", status.type, status);
      if (status.type === "ready") {
        setStatus("WASM analysis engine ready.");
      } else if (status.type === "fatal") {
        console.error("[IDE] WASM fatal:", status.error);
        setStatus("WASM analysis unavailable: " + status.error);
      } else if (status.type === "restarting") {
        bumpTelemetry("worker_restarts");
      }
    });
    await wasmClient.ready();
    console.log("[IDE] WASM analysis client ready");
    return true;
  } catch (error) {
    console.error("[IDE] WASM analysis init failed:", error);
    setStatus("WASM analysis init failed: " + String(error.message || error));
    wasmClient = null;
    return false;
  }
}

function syncDocumentsToWasm() {
  if (!wasmClient) {
    return;
  }
  const documents = [];
  for (const [path, tab] of state.openTabs) {
    if (isStructuredTextPath(path)) {
      documents.push({ uri: path, text: tab.content });
    }
  }
  if (documents.length === 0) {
    return;
  }
  wasmClient.applyDocuments(documents).catch((error) => {
    console.warn("[IDE] WASM document sync failed:", error);
  });
}

function editorText() {
  return state.editorView ? state.editorView.getValue() : "";
}

function setActiveContent(content) {
  if (!state.editorView) {
    return;
  }
  const current = state.editorView.getValue();
  if (current === content) {
    return;
  }
  state.suppressEditorChange = true;
  state.editorView.setValue(content);
  state.suppressEditorChange = false;
}

function updateCursorLabel() {
  if (!state.editorView) {
    return;
  }
  const pos = fromMonacoPosition(state.editorView.getPosition());
  el.cursorLabel.textContent = `Ln ${pos.line + 1}, Col ${pos.character + 1}`;
}

function cursorPosition() {
  if (!state.editorView) {
    return null;
  }
  return fromMonacoPosition(state.editorView.getPosition());
}

function applyMonacoMarkers(items, model) {
  if (!monaco || !model) {
    return;
  }
  const markers = Array.isArray(items)
    ? items.map((item) => {
      const range = toMonacoRange(item.range || {}, model);
      return {
        startLineNumber: range.startLineNumber,
        startColumn: range.startColumn,
        endLineNumber: range.endLineNumber,
        endColumn: Math.max(range.startColumn + 1, range.endColumn),
        severity: monacoMarkerSeverity(item.severity),
        message: item.message || "diagnostic",
        code: item.code ? String(item.code) : undefined,
      };
    })
    : [];
  monaco.editor.setModelMarkers(model, MONACO_MARKER_OWNER, markers);
}

function setModelLanguageForPath(model, path) {
  if (!monaco || !model) {
    return;
  }
  monaco.editor.setModelLanguage(model, monacoLanguageForPath(path));
}

function disposeEditorDisposables() {
  for (const disposable of state.editorDisposables) {
    try {
      disposable.dispose();
    } catch {
      // no-op
    }
  }
  state.editorDisposables = [];
}

function scheduleAutoCompletionTrigger() {
  if (completionTriggerTimer) {
    clearTimeout(completionTriggerTimer);
    completionTriggerTimer = null;
  }
  completionTriggerTimer = setTimeout(() => {
    startCompletion();
  }, 120);
}

function maybeTriggerCompletionOnEdit(event) {
  const tab = activeTab();
  if (!tab || !isStructuredTextPath(tab.path) || !state.editorView) {
    return;
  }
  if (!Array.isArray(event?.changes) || event.changes.length !== 1) {
    return;
  }
  const change = event.changes[0];
  if (!change || typeof change.text !== "string") {
    return;
  }
  if (change.text.length !== 1) {
    return;
  }
  if (!/[A-Za-z0-9_.]/.test(change.text)) {
    return;
  }
  scheduleAutoCompletionTrigger();
}

function clearHoverPopupTimer() {
  if (cursorHoverPopupTimer) {
    clearTimeout(cursorHoverPopupTimer);
    cursorHoverPopupTimer = null;
  }
}

function scheduleHoverPopupOnMouse(event) {
  clearHoverPopupTimer();
  const tab = activeTab();
  if (!tab || !isStructuredTextPath(tab.path) || !state.editorView) {
    return;
  }
  const target = event?.target;
  const position = target?.position;
  if (!position) {
    return;
  }
  if (monaco?.editor?.MouseTargetType && typeof target?.type === "number") {
    const type = target.type;
    const allowed = new Set([
      monaco.editor.MouseTargetType.CONTENT_TEXT,
      monaco.editor.MouseTargetType.CONTENT_EMPTY,
    ]);
    if (!allowed.has(type)) {
      return;
    }
  }
  cursorHoverPopupTimer = setTimeout(() => {
    if (!state.editorView) {
      return;
    }
    state.editorView.trigger("mouse", "editor.action.showHover", {
      lineNumber: position.lineNumber,
      column: position.column,
    });
  }, 260);
}

function scheduleDocumentHighlight(editor) {
  if (documentHighlightTimer) {
    clearTimeout(documentHighlightTimer);
    documentHighlightTimer = null;
  }
  documentHighlightTimer = setTimeout(() => {
    updateDocumentHighlights(editor);
  }, 150);
}

async function updateDocumentHighlights(editor) {
  if (!wasmClient || !editor) {
    return;
  }
  const model = editor.getModel();
  if (!model) {
    documentHighlightDecorations = editor.deltaDecorations(documentHighlightDecorations, []);
    return;
  }
  const tab = activeTab();
  if (!tab || !isStructuredTextPath(tab.path)) {
    documentHighlightDecorations = editor.deltaDecorations(documentHighlightDecorations, []);
    return;
  }
  const position = fromMonacoPosition(editor.getPosition());
  try {
    const highlights = await wasmClient.documentHighlight(tab.path, position);
    if (!Array.isArray(highlights) || highlights.length === 0) {
      documentHighlightDecorations = editor.deltaDecorations(documentHighlightDecorations, []);
      return;
    }
    const decorations = highlights.map((h) => ({
      range: toMonacoRange(h.range, model),
      options: {
        className: h.kind === "write" ? "ide-document-highlight-write" : "ide-document-highlight-read",
        overviewRuler: {color: "#14b8a680", position: monaco.editor.OverviewRulerLane.Center},
      },
    }));
    documentHighlightDecorations = editor.deltaDecorations(documentHighlightDecorations, decorations);
  } catch {
    documentHighlightDecorations = editor.deltaDecorations(documentHighlightDecorations, []);
  }
}

function createEditor(initialContent, path) {
  const model = monaco.editor.createModel(initialContent, monacoLanguageForPath(path));
  const view = monaco.editor.create(el.editorMount, {
    model,
    readOnly: !state.writeEnabled,
    automaticLayout: true,
    minimap: {enabled: true, scale: 1, showSlider: "mouseover"},
    lineNumbers: "on",
    scrollBeyondLastLine: false,
    fontFamily: "JetBrains Mono, Fira Code, IBM Plex Mono, monospace",
    fontSize: 13,
    lineHeight: 20,
    tabSize: 2,
    insertSpaces: true,
    quickSuggestions: {other: true, comments: false, strings: true},
    quickSuggestionsDelay: 120,
    suggestOnTriggerCharacters: true,
    wordBasedSuggestions: "off",
    parameterHints: {enabled: true},
    snippetSuggestions: "inline",
    hover: {enabled: "on", delay: 250, sticky: true},
    occurrencesHighlight: "singleFile",
    selectionHighlight: true,
    bracketPairColorization: {enabled: true},
    smoothScrolling: true,
    renderLineHighlight: "all",
    padding: {top: 8, bottom: 8},
    theme: document.body.dataset.theme === "dark" ? "trust-dark" : "trust-light",
  });

  disposeEditorDisposables();

  state.editorDisposables.push(view.onDidChangeModelContent((event) => {
    if (state.suppressEditorChange) {
      return;
    }
    const tab = activeTab();
    if (!tab) {
      return;
    }
    tab.content = view.getValue();
    const dirty = tab.content !== tab.savedContent;
    markTabDirty(tab.path, dirty);
    updateDraftInfo();
    if (dirty) {
      const draftStored = saveDraft(tab.path, tab.content);
      scheduleAutosave();
      if (draftStored) {
        updateSaveBadge(state.online ? "warn" : "err", state.online ? "dirty" : "offline draft");
      }
    } else {
      clearDraft(tab.path);
      updateSaveBadge("ok", "saved");
    }
    syncSecondaryEditor();
    updateCursorLabel();
    syncDocumentsToWasm();
    scheduleDiagnostics();
    maybeTriggerCompletionOnEdit(event);
  }));

  state.editorDisposables.push(view.onDidType((text) => {
    const tab = activeTab();
    if (!tab || !isStructuredTextPath(tab.path)) {
      return;
    }
    const char = String(text || "").slice(-1);
    if (/[A-Za-z0-9_.]/.test(char)) {
      scheduleAutoCompletionTrigger();
    }
  }));

  state.editorDisposables.push(view.onDidChangeCursorPosition((event) => {
    updateCursorLabel();
    scheduleCursorInsights(fromMonacoPosition(event.position));
    scheduleDocumentHighlight(view);
  }));

  state.editorDisposables.push(view.onMouseMove((event) => {
    scheduleHoverPopupOnMouse(event);
  }));

  state.editorDisposables.push(view.onMouseLeave(() => {
    clearHoverPopupTimer();
  }));

  view.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
    saveActiveTab({explicit: true}).catch(() => {});
  });
  view.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Space, () => {
    startCompletion();
  });
  view.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyP, () => {
    openCommandPalette();
  });

  return view;
}

function createSecondaryEditor(initialContent, path) {
  const model = monaco.editor.createModel(initialContent, monacoLanguageForPath(path));
  return monaco.editor.create(el.editorMountSecondary, {
    model,
    readOnly: true,
    automaticLayout: true,
    minimap: {enabled: false},
    lineNumbers: "on",
    scrollBeyondLastLine: false,
    fontFamily: "JetBrains Mono, Fira Code, IBM Plex Mono, monospace",
    fontSize: 13,
    lineHeight: 20,
    renderLineHighlight: "none",
    padding: {top: 8, bottom: 8},
    theme: document.body.dataset.theme === "dark" ? "trust-dark" : "trust-light",
  });
}

function setSecondaryEditorContent(text, path) {
  if (!state.secondaryEditorView) {
    state.secondaryEditorView = createSecondaryEditor(text, path);
    return;
  }
  setModelLanguageForPath(state.secondaryEditorView.getModel(), path);
  const current = state.secondaryEditorView.getValue();
  if (current === text) {
    return;
  }
  state.secondaryEditorView.setValue(text);
}

function setActivePane(pane) {
  state.activePane = pane;
  el.editorPanePrimary.classList.toggle("pane-active", pane === "primary");
  el.editorPaneSecondary.classList.toggle("pane-active", pane === "secondary");
}

function syncSecondaryEditor() {
  if (!state.splitEnabled || !state.editorView) {
    return;
  }
  const path = state.secondaryPath;
  if (!path) {
    return;
  }
  const tab = state.openTabs.get(path);
  if (tab) {
    setSecondaryEditorContent(tab.content, tab.path);
  }
}

function openInSecondaryPane(path, content) {
  const tab = state.openTabs.get(path);
  if (!tab && !content) {
    return;
  }
  state.secondaryPath = path;
  state.secondaryOpenTabs.add(path);
  setSecondaryEditorContent(content || tab.content, path);
}

function toggleSplitEditor() {
  state.splitEnabled = !state.splitEnabled;
  el.editorGrid.classList.toggle("split", state.splitEnabled);
  el.editorPaneSecondary.classList.toggle("ide-hidden", !state.splitEnabled);
  el.splitBtn.setAttribute("aria-label", state.splitEnabled ? "Single editor" : "Toggle split editor");
  el.splitBtn.title = state.splitEnabled ? "Single" : "Split";
  if (state.splitEnabled) {
    // Show per-pane tab bars, hide the shared tab bar
    el.tabBar.classList.add("ide-hidden");
    el.tabBarPrimary.classList.remove("ide-hidden");

    setActivePane("primary");
    if (!state.secondaryPath || state.secondaryPath === state.activePath) {
      for (const [p] of state.openTabs) {
        if (p !== state.activePath) {
          state.secondaryPath = p;
          break;
        }
      }
    }
    // Seed secondary tab set
    if (state.secondaryPath) {
      state.secondaryOpenTabs.add(state.secondaryPath);
    }
    syncSecondaryEditor();
    renderTabs();
  } else {
    // Restore shared tab bar, hide per-pane tab bars
    el.tabBar.classList.remove("ide-hidden");
    el.tabBarPrimary.classList.add("ide-hidden");

    // Merge secondary tabs back into shared openTabs (they already share the Map)
    state.secondaryOpenTabs.clear();
    setActivePane("primary");
    renderTabs();
  }
}

function setEditorContent(text) {
  if (!state.editorView) {
    return;
  }
  const current = state.editorView.getValue();
  if (current === text) {
    return;
  }
  state.suppressEditorChange = true;
  state.editorView.setValue(text);
  state.suppressEditorChange = false;
  syncSecondaryEditor();
  scheduleDiagnostics({immediate: true});
}

// ── UI Rendering (tree, tabs, breadcrumbs, panels) ─────

function flattenFiles(nodes, out = []) {
  for (const node of nodes) {
    if (node.kind === "file") {
      out.push(node.path);
    } else if (Array.isArray(node.children)) {
      flattenFiles(node.children, out);
    }
  }
  return out;
}

function nodeKindForPath(path, nodes = state.tree) {
  for (const node of nodes || []) {
    if (node.path === path) {
      return node.kind || null;
    }
    if (node.kind === "directory" && Array.isArray(node.children)) {
      const nested = nodeKindForPath(path, node.children);
      if (nested) {
        return nested;
      }
    }
  }
  return null;
}

function nodeMatchesFilter(node, filter) {
  if (!filter) {
    return true;
  }
  const name = String(node.name || "").toLowerCase();
  const path = String(node.path || "").toLowerCase();
  if (name.includes(filter) || path.includes(filter)) {
    return true;
  }
  return Array.isArray(node.children) && node.children.some((child) => nodeMatchesFilter(child, filter));
}

function selectPath(path) {
  state.selectedPath = path || null;
  renderFileTree();
}

function toggleDir(path) {
  if (state.expandedDirs.has(path)) {
    state.expandedDirs.delete(path);
  } else {
    state.expandedDirs.add(path);
  }
  renderFileTree();
}

function closeTreeContextMenu() {
  el.treeContextMenu.classList.add("ide-hidden");
  state.contextPath = null;
}

function openTreeContextMenu(path, x, y) {
  state.contextPath = path;
  selectPath(path);
  const writable = Boolean(state.writeEnabled);
  el.ctxNewFileBtn.disabled = !writable;
  el.ctxNewFolderBtn.disabled = !writable;
  el.ctxRenameBtn.disabled = !writable;
  el.ctxDeleteBtn.disabled = !writable;
  el.treeContextMenu.style.left = `${Math.max(8, Math.floor(x))}px`;
  el.treeContextMenu.style.top = `${Math.max(8, Math.floor(y))}px`;
  el.treeContextMenu.classList.remove("ide-hidden");
}

function renderTreeNode(node, depth) {
  if (!nodeMatchesFilter(node, state.fileFilter)) {
    return;
  }
  const row = document.createElement("button");
  row.type = "button";
  row.className = "ide-tree-row";
  row.setAttribute("role", "treeitem");
  row.style.paddingLeft = `${8 + depth * 14}px`;
  const isSelected = state.selectedPath === node.path || state.activePath === node.path;
  if (isSelected) {
    row.setAttribute("aria-current", "true");
  }

  const indent = document.createElement("span");
  indent.className = "ide-tree-indent";
  indent.textContent = "";
  row.appendChild(indent);

  const icon = document.createElement("span");
  icon.className = "ide-tree-icon";
  if (node.kind === "directory") {
    const expanded = state.expandedDirs.has(node.path) || state.fileFilter.length > 0;
    icon.classList.add(expanded ? "folder-open" : "folder-closed");
  } else {
    const ext = String(node.name || "").split(".").pop().toLowerCase();
    const iconMap = {st: "file-st", toml: "file-toml", md: "file-md", json: "file-json"};
    icon.classList.add(iconMap[ext] || "file-generic");
  }
  row.appendChild(icon);

  const label = document.createElement("span");
  label.textContent = node.name;
  row.appendChild(label);

  row.addEventListener("click", async () => {
    closeTreeContextMenu();
    selectPath(node.path);
    if (node.kind === "directory") {
      toggleDir(node.path);
    } else {
      await openFile(node.path);
    }
  });
  row.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    openTreeContextMenu(node.path, event.clientX, event.clientY);
  });
  el.fileTree.appendChild(row);

  if (node.kind === "directory" && (state.expandedDirs.has(node.path) || state.fileFilter.length > 0)) {
    for (const child of node.children || []) {
      renderTreeNode(child, depth + 1);
    }
  }
}

function renderFileTree() {
  el.fileTree.innerHTML = "";
  if (state.tree.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = state.activeProject
      ? "No visible files in project root."
      : "No project selected. Use Open Folder.";
    el.fileTree.appendChild(empty);
    return;
  }
  for (const node of state.tree) {
    renderTreeNode(node, 0);
  }
}

function renderTabs() {
  if (state.splitEnabled) {
    renderPrimaryTabs();
    renderSecondaryTabs();
  } else {
    // Single-editor mode: render into the shared tab bar
    el.tabBar.innerHTML = "";
    for (const [path, tab] of state.openTabs.entries()) {
      el.tabBar.appendChild(createTabButton(path, tab, path === state.activePath, async () => {
        await switchTab(path);
      }));
    }
  }
}

function createTabButton(path, tab, isActive, onClick) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = `ide-tab${isActive ? " active" : ""}`;
  button.setAttribute("aria-label", `Open tab ${path}`);
  if (tab.dirty) {
    const dot = document.createElement("span");
    dot.className = "dirty-dot";
    button.appendChild(dot);
  }
  const label = document.createElement("span");
  label.textContent = path;
  button.appendChild(label);
  button.addEventListener("click", onClick);
  return button;
}

function renderPrimaryTabs() {
  el.tabBarPrimary.innerHTML = "";
  for (const [path, tab] of state.openTabs.entries()) {
    el.tabBarPrimary.appendChild(createTabButton(path, tab, path === state.activePath, async () => {
      await switchTab(path);
    }));
  }
}

function renderSecondaryTabs() {
  el.tabBarSecondary.innerHTML = "";
  for (const path of state.secondaryOpenTabs) {
    const tab = state.openTabs.get(path);
    if (!tab) continue;
    el.tabBarSecondary.appendChild(createTabButton(path, tab, path === state.secondaryPath, async () => {
      openInSecondaryPane(path, tab.content);
      renderSecondaryTabs();
    }));
  }
}

function renderBreadcrumbs(path) {
  el.breadcrumbBar.innerHTML = "";
  const projectRoot = state.activeProject || "project";
  const rootLabel = projectRoot.split("/").filter(Boolean).pop() || projectRoot;
  if (!path) {
    el.breadcrumbBar.textContent = rootLabel;
    return;
  }
  const parts = String(path).split("/").filter(Boolean);
  const root = document.createElement("span");
  root.textContent = rootLabel;
  el.breadcrumbBar.appendChild(root);
  for (const [index, part] of parts.entries()) {
    const sep = document.createElement("span");
    sep.className = "sep";
    sep.textContent = "\u203A";
    el.breadcrumbBar.appendChild(sep);

    const item = document.createElement("span");
    item.textContent = part;
    if (index === parts.length - 1) {
      item.className = "current";
    }
    el.breadcrumbBar.appendChild(item);
  }
}

function markTabDirty(path, dirty) {
  const tab = state.openTabs.get(path);
  if (!tab) {
    return;
  }
  tab.dirty = dirty;
  renderTabs();
}

function updateDraftInfo() {
  const dirtyTabs = [...state.openTabs.values()].filter((tab) => tab.dirty).length;
  if (dirtyTabs === 0) {
    el.draftInfo.textContent = "Draft sync idle";
    return;
  }
  el.draftInfo.textContent = `${dirtyTabs} unsynced draft(s)`;
}

function diagnosticsToProblems(items) {
  el.problemsPanel.innerHTML = "";
  if (!items || items.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = "No diagnostics.";
    el.problemsPanel.appendChild(empty);
    return;
  }

  for (const item of items) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "ide-problem";
    row.setAttribute("aria-label", `Diagnostic ${item.message}`);

    const title = document.createElement("p");
    title.className = "ide-problem-title";
    const severity = String(item.severity || "warning").toLowerCase().includes("error")
      ? "error"
      : "warning";
    const chip = document.createElement("span");
    chip.className = `ide-severity ${severity}`;
    chip.textContent = severity;
    title.appendChild(chip);
    title.appendChild(document.createTextNode(` ${item.message}`));

    const meta = document.createElement("p");
    meta.className = "ide-problem-meta";
    meta.textContent = `${item.code || "diag"} at ${item.range.start.line}:${item.range.start.character}`;

    row.appendChild(title);
    row.appendChild(meta);
    row.addEventListener("click", () => {
      if (!state.editorView) {
        return;
      }
      const model = activeModel();
      if (!model) {
        return;
      }
      const pos = toMonacoPosition(item.range.start, model);
      state.editorView.setPosition(pos);
      state.editorView.revealPositionInCenter(pos);
      state.editorView.focus();
      updateCursorLabel();
    });

    el.problemsPanel.appendChild(row);
  }
}

function jumpToRange(range) {
  if (!state.editorView || !range || !range.start) {
    return;
  }
  const model = activeModel();
  if (!model) {
    return;
  }
  const monacoRange = toMonacoRange(range, model);
  state.editorView.setSelection(monacoRange);
  state.editorView.revealRangeInCenter(monacoRange);
  state.editorView.focus();
  updateCursorLabel();
}

function renderReferences(references) {
  el.referencesPanel.innerHTML = "";
  if (!Array.isArray(references) || references.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = "No references.";
    el.referencesPanel.appendChild(empty);
    return;
  }
  for (const location of references.slice(0, 80)) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ide-link-button";
    const refPath = location.uri || location.path;
    const start = location.range?.start || {line: 0, character: 0};
    const writeTag = location.is_write ? " [write]" : "";
    button.textContent = `${refPath}:${start.line + 1}:${start.character + 1}${writeTag}`;
    button.addEventListener("click", async () => {
      await openFile(refPath);
      jumpToRange(location.range);
    });
    el.referencesPanel.appendChild(button);
  }
}

function renderSearchHits(hits) {
  el.searchPanel.innerHTML = "";
  if (!Array.isArray(hits) || hits.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = "No search results.";
    el.searchPanel.appendChild(empty);
    return;
  }
  for (const hit of hits.slice(0, 100)) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ide-link-button";
    button.textContent = `${hit.path}:${Number(hit.line) + 1}  ${hit.preview || ""}`;
    button.addEventListener("click", async () => {
      await openFile(hit.path);
      jumpToRange({
        start: {line: Number(hit.line || 0), character: Number(hit.character || 0)},
      });
    });
    el.searchPanel.appendChild(button);
  }
}

function renderSymbolHits(hits) {
  el.searchPanel.innerHTML = "";
  if (!Array.isArray(hits) || hits.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = "No symbols.";
    el.searchPanel.appendChild(empty);
    return;
  }
  for (const hit of hits.slice(0, 120)) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ide-link-button";
    button.textContent = `${hit.kind || "symbol"}  ${hit.name || ""}  (${hit.path}:${Number(hit.line || 0) + 1})`;
    button.addEventListener("click", async () => {
      await openFile(hit.path);
      jumpToRange({
        start: {line: Number(hit.line || 0), character: Number(hit.character || 0)},
      });
    });
    el.searchPanel.appendChild(button);
  }
}

// ── Project Management ─────────────────────────────────

function applyProjectSelection(selection) {
  const active = selection?.active_project ? String(selection.active_project) : "";
  const startup = selection?.startup_project ? String(selection.startup_project) : "";
  state.activeProject = active || null;
  state.startupProject = startup || null;

  const projectPath = state.activeProject || state.startupProject || "";
  const projectName = projectPath ? projectPath.split("/").filter(Boolean).pop() || projectPath : "";
  el.ideTitle.textContent = projectName || "truST IDE";
  el.statusProject.textContent = state.activeProject || "--";
  if (state.activeProject) {
    const shortName = state.activeProject.split("/").filter(Boolean).pop() || state.activeProject;
    el.scopeNote.textContent = shortName;
  } else {
    el.scopeNote.textContent = "No project";
  }
}

async function refreshProjectSelection() {
  const selection = await apiJson("/api/ide/project", {
    method: "GET",
    headers: apiHeaders(),
  });
  applyProjectSelection(selection || {});
  return selection;
}

function loadRecentProjects() {
  try {
    const raw = localStorage.getItem(RECENT_PROJECTS_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveRecentProject(path) {
  const recent = loadRecentProjects().filter((item) => item.path !== path);
  recent.unshift({path, ts: Date.now()});
  if (recent.length > MAX_RECENT_PROJECTS) recent.length = MAX_RECENT_PROJECTS;
  try {
    localStorage.setItem(RECENT_PROJECTS_KEY, JSON.stringify(recent));
  } catch {
    // quota exceeded
  }
}

function renderRecentProjects(onSelect) {
  const recent = loadRecentProjects();
  el.openProjectRecent.innerHTML = "";
  if (recent.length === 0) {
    const hint = document.createElement("div");
    hint.className = "muted";
    hint.style.padding = "6px 0";
    hint.textContent = "No recent projects. Enter a path above.";
    el.openProjectRecent.appendChild(hint);
    return;
  }
  state._recentItems = [];
  for (const item of recent) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "ide-recent-item";
    row.innerHTML = `<svg viewBox="0 0 16 16"><path d="M2 13V4a1 1 0 0 1 1-1h3.5l2 2H13a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/></svg>`;
    const label = document.createElement("span");
    label.textContent = item.path;
    row.appendChild(label);
    const ts = document.createElement("span");
    ts.className = "recent-ts";
    ts.textContent = item.ts ? new Date(item.ts).toLocaleDateString() : "";
    row.appendChild(ts);
    row.addEventListener("click", () => onSelect(item.path));
    el.openProjectRecent.appendChild(row);
    state._recentItems.push(row);
  }
}

function openProjectPanel() {
  state._recentSelectedIndex = -1;
  el.openProjectInput.value = state.activeProject || state.startupProject || "";
  renderRecentProjects((path) => {
    closeOpenProjectPanel();
    doOpenProject(path);
  });
  hideBrowseListing();
  el.openProjectPanel.classList.add("open");
  el.openProjectInput.focus();
  el.openProjectInput.select();
}

function closeOpenProjectPanel() {
  el.openProjectPanel.classList.remove("open");
  state._recentSelectedIndex = -1;
  hideBrowseListing();
}

async function doOpenProject(pathStr) {
  const path = String(pathStr || "").trim();
  if (!path) return;
  const selection = await apiJson("/api/ide/project/open", {
    method: "POST",
    headers: apiHeaders(),
    body: JSON.stringify({path}),
  });
  applyProjectSelection(selection || {});
  saveRecentProject(state.activeProject || path);

  state.tree = [];
  state.files = [];
  state.openTabs.clear();
  state.secondaryOpenTabs.clear();
  state.activePath = null;
  state.selectedPath = null;
  state.secondaryPath = null;
  state.references = [];
  state.searchHits = [];
  showWelcomeScreen();
  renderFileTree();
  renderTabs();
  renderBreadcrumbs(null);
  renderReferences([]);
  renderSearchHits([]);
  if (state.editorView) {
    state.suppressEditorChange = true;
    state.editorView.setValue("");
    state.suppressEditorChange = false;
    applyMonacoMarkers([], activeModel());
  }
  updateDraftInfo();
  setStatus(`Opened project: ${state.activeProject || path}`);
  await bootstrapFiles();
}

async function openProjectFlow() {
  openProjectPanel();
}

async function bootstrapFiles() {
  if (!state.activeProject) {
    state.tree = [];
    state.files = [];
    renderFileTree();
    renderBreadcrumbs(null);
    return;
  }
  let result;
  try {
    result = await apiJson("/api/ide/tree", {
      method: "GET",
      headers: apiHeaders(),
    });
  } catch (error) {
    const message = String(error?.message || error).toLowerCase();
    if (message.includes("project root unavailable")) {
      applyProjectSelection({active_project: null, startup_project: state.startupProject});
      state.tree = [];
      state.files = [];
      renderFileTree();
      renderBreadcrumbs(null);
      setStatus("No project selected. Use Open Folder.");
      return;
    }
    throw error;
  }
  state.tree = Array.isArray(result.tree) ? result.tree : [];
  state.files = flattenFiles(state.tree, []).sort((a, b) => a.localeCompare(b));
  renderFileTree();
  if (!state.activePath && state.files.length > 0) {
    await openFile(state.files[0]);
  } else if (!state.activePath) {
    renderBreadcrumbs(null);
  }
}

// ── File Operations ────────────────────────────────────

function activeTab() {
  if (!state.activePath) {
    return null;
  }
  return state.openTabs.get(state.activePath) || null;
}

function saveDraft(path, content) {
  try {
    localStorage.setItem(`${DRAFT_PREFIX}${path}`, content);
    return true;
  } catch (error) {
    const message = String(error?.message || error);
    bumpTelemetry("autosave_failures");
    updateSaveBadge("err", "draft full");
    setStatus(`Local draft storage failed: ${message}`);
    return false;
  }
}

function loadDraft(path) {
  return localStorage.getItem(`${DRAFT_PREFIX}${path}`);
}

function clearDraft(path) {
  localStorage.removeItem(`${DRAFT_PREFIX}${path}`);
}

async function saveActiveTab({explicit = false} = {}) {
  const tab = activeTab();
  if (!tab) {
    return;
  }
  if (!state.writeEnabled || tab.readOnly) {
    updateSaveBadge("warn", "read-only");
    return;
  }
  const latestContent = state.editorView.getValue();
  tab.content = latestContent;
  if (tab.content === tab.savedContent && !explicit) {
    updateSaveBadge("ok", "saved");
    return;
  }

  if (!state.online) {
    updateSaveBadge("err", "offline draft");
    saveDraft(tab.path, tab.content);
    updateDraftInfo();
    return;
  }

  updateSaveBadge("warn", "saving...");
  try {
    const result = await apiJson("/api/ide/file", {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify({
        path: tab.path,
        expected_version: tab.version,
        content: tab.content,
      }),
    });
    tab.version = result.version;
    tab.savedContent = tab.content;
    tab.dirty = false;
    clearDraft(tab.path);
    renderTabs();
    updateDraftInfo();
    updateSaveBadge("ok", "saved");
    if (state.lastFailedAction?.kind === "save") {
      setRetryAction(null, `Saved ${tab.path}`);
    } else {
      setStatus(`Saved ${tab.path}`);
    }
  } catch (error) {
    const message = String(error.message || error);
    if (message.includes("current version")) {
      updateSaveBadge("err", "conflict");
      setRetryAction({kind: "save", path: tab.path}, `Save conflict on ${tab.path}. Retry after merge/reload.`);
    } else {
      bumpTelemetry("autosave_failures");
      updateSaveBadge("err", "save failed");
      setRetryAction({kind: "save", path: tab.path}, `Save failed: ${message}`);
    }
    saveDraft(tab.path, tab.content);
    updateDraftInfo();
  }
}

function scheduleAutosave() {
  if (state.autosaveTimer) {
    clearTimeout(state.autosaveTimer);
  }
  state.autosaveTimer = setTimeout(() => {
    saveActiveTab().catch(() => {});
  }, 800);
}

async function flushDirtyTabs() {
  for (const [path, tab] of state.openTabs.entries()) {
    if (!tab.dirty) {
      continue;
    }
    const prev = state.activePath;
    if (path !== state.activePath) {
      await switchTab(path, {preserveSelection: true});
    }
    await saveActiveTab();
    if (prev && prev !== state.activePath) {
      await switchTab(prev, {preserveSelection: true});
    }
  }
}

async function formatActiveDocument() {
  const tab = activeTab();
  if (!tab || !state.editorView) {
    return;
  }
  if (!isStructuredTextPath(tab.path)) {
    setStatus("Format document is available for .st files.");
    return;
  }
  const result = await apiJson("/api/ide/format", {
    method: "POST",
    headers: apiHeaders(),
    body: JSON.stringify({
      path: tab.path,
      content: editorText(),
    }),
    timeoutMs: 2500,
  });
  if (!result || typeof result.content !== "string") {
    setStatus("Format did not return document content.");
    return;
  }
  setEditorContent(result.content);
  const currentTab = activeTab();
  if (currentTab) {
    currentTab.content = result.content;
    const dirty = currentTab.content !== currentTab.savedContent;
    markTabDirty(currentTab.path, dirty);
    updateDraftInfo();
    if (dirty) {
      saveDraft(currentTab.path, currentTab.content);
      updateSaveBadge("warn", "dirty");
    } else {
      clearDraft(currentTab.path);
      updateSaveBadge("ok", "saved");
    }
  }
  setStatus(result.changed ? `Formatted ${tab.path}` : `No formatting changes for ${tab.path}`);
}

function parentDirectory(path) {
  const parts = String(path || "").split("/").filter(Boolean);
  if (parts.length <= 1) {
    return "";
  }
  parts.pop();
  return parts.join("/");
}

function selectedDirectory() {
  if (state.selectedPath) {
    const selectedNode = state.selectedPath;
    const kind = nodeKindForPath(selectedNode);
    if (kind === "file") {
      return parentDirectory(selectedNode);
    }
    if (kind === "directory") {
      return selectedNode;
    }
  }
  if (state.activePath) {
    return parentDirectory(state.activePath);
  }
  return "";
}

function remapOpenTabs(oldPath, newPath, isDirectory) {
  const next = new Map();
  for (const [path, tab] of state.openTabs.entries()) {
    if (path === oldPath || (isDirectory && path.startsWith(`${oldPath}/`))) {
      const suffix = path.slice(oldPath.length);
      const mapped = `${newPath}${suffix}`;
      next.set(mapped, {...tab, path: mapped});
    } else {
      next.set(path, tab);
    }
  }
  state.openTabs = next;
  // Remap secondaryOpenTabs
  const nextSecondary = new Set();
  for (const path of state.secondaryOpenTabs) {
    if (path === oldPath || (isDirectory && path.startsWith(`${oldPath}/`))) {
      const suffix = path.slice(oldPath.length);
      nextSecondary.add(`${newPath}${suffix}`);
    } else {
      nextSecondary.add(path);
    }
  }
  state.secondaryOpenTabs = nextSecondary;
  if (state.activePath === oldPath || (isDirectory && state.activePath?.startsWith(`${oldPath}/`))) {
    const suffix = state.activePath.slice(oldPath.length);
    state.activePath = `${newPath}${suffix}`;
  }
  if (state.secondaryPath === oldPath || (isDirectory && state.secondaryPath?.startsWith(`${oldPath}/`))) {
    const suffix = state.secondaryPath.slice(oldPath.length);
    state.secondaryPath = `${newPath}${suffix}`;
  }
}

function removeTabsForPath(path, isDirectory) {
  for (const key of [...state.openTabs.keys()]) {
    if (key === path || (isDirectory && key.startsWith(`${path}/`))) {
      state.openTabs.delete(key);
      state.secondaryOpenTabs.delete(key);
    }
  }
  if (state.activePath === path || (isDirectory && state.activePath?.startsWith(`${path}/`))) {
    state.activePath = null;
  }
  if (state.secondaryPath === path || (isDirectory && state.secondaryPath?.startsWith(`${path}/`))) {
    state.secondaryPath = null;
  }
}

async function createPath(kind) {
  const base = selectedDirectory();
  const defaultPath = kind === "directory"
    ? (base ? `${base}/new_folder` : "new_folder")
    : (base ? `${base}/new_file.st` : "new_file.st");
  const input = await idePrompt(kind === "directory" ? "Create folder path:" : "Create file path:", defaultPath);
  if (!input) {
    return;
  }
  const payload = {
    path: input.trim(),
    kind,
  };
  if (kind === "file") {
    payload.content = "";
  }
  await apiJson("/api/ide/fs/create", {
    method: "POST",
    headers: apiHeaders(),
    body: JSON.stringify(payload),
  });
  setStatus(`${kind === "directory" ? "Folder" : "File"} created: ${payload.path}`);
  await bootstrapFiles();
  if (kind === "file") {
    selectPath(payload.path);
    await openFile(payload.path);
  } else {
    selectPath(payload.path);
    state.expandedDirs.add(payload.path);
    renderFileTree();
  }
}

async function renameSelectedPath() {
  const sourcePath = state.selectedPath || state.activePath;
  if (!sourcePath) {
    setStatus("Select a file or folder first.");
    return;
  }
  const nextPath = await idePrompt("Rename/move path to:", sourcePath);
  if (!nextPath || nextPath.trim() === sourcePath) {
    return;
  }
  const result = await apiJson("/api/ide/fs/rename", {
    method: "POST",
    headers: apiHeaders(),
    body: JSON.stringify({
      path: sourcePath,
      new_path: nextPath.trim(),
    }),
  });
  const isDirectory = result.kind === "directory";
  remapOpenTabs(sourcePath, result.path, isDirectory);
  selectPath(result.path);
  setStatus(`Renamed: ${sourcePath} -> ${result.path}`);
  await bootstrapFiles();
  if (state.activePath && state.openTabs.has(state.activePath)) {
    await switchTab(state.activePath, {preserveSelection: true});
  } else if (state.files.length > 0) {
    await openFile(state.files[0]);
  }
}

async function deleteSelectedPath() {
  const path = state.selectedPath || state.activePath;
  if (!path) {
    setStatus("Select a file or folder first.");
    return;
  }
  const confirmed = await ideConfirm("Delete", `Delete ${path}?`);
  if (!confirmed) {
    return;
  }
  const isDirectory = nodeKindForPath(path) !== "file";
  await apiJson("/api/ide/fs/delete", {
    method: "POST",
    headers: apiHeaders(),
    body: JSON.stringify({path}),
  });
  removeTabsForPath(path, isDirectory);
  selectPath(null);
  setStatus(`Deleted: ${path}`);
  await bootstrapFiles();
  if (!state.activePath && state.files.length > 0) {
    await openFile(state.files[0]);
  } else {
    renderTabs();
  }
}

async function openFile(path, {targetPane} = {}) {
  const pane = targetPane || state.activePane;

  // Ensure the file is loaded into openTabs
  if (!state.openTabs.has(path)) {
    setStatus(`Opening ${path}...`);
    const snapshot = await apiJson(`/api/ide/file?path=${encodeURIComponent(path)}`, {
      method: "GET",
      headers: apiHeaders(),
    });
    const draft = loadDraft(path);
    const content = draft ?? snapshot.content;
    state.openTabs.set(path, {
      path,
      version: Number(snapshot.version),
      savedContent: snapshot.content,
      content,
      dirty: draft !== null && draft !== snapshot.content,
      readOnly: Boolean(snapshot.read_only),
    });
    syncDocumentsToWasm();
  }

  // Route to the correct pane
  if (state.splitEnabled && pane === "secondary") {
    const tab = state.openTabs.get(path);
    openInSecondaryPane(path, tab.content);
    renderTabs();
    return;
  }

  await switchTab(path);
}

function showWelcomeScreen() {
  el.editorWelcome.style.display = "";
  el.editorGrid.style.display = "none";
}

async function switchTab(path, {preserveSelection = false} = {}) {
  const tab = state.openTabs.get(path);
  if (!tab) {
    return;
  }

  if (state.activePath && state.editorView) {
    const previous = state.openTabs.get(state.activePath);
    if (previous) {
      previous.content = state.editorView.getValue();
    }
  }

  state.activePath = path;
  state.selectedPath = path;
  renderBreadcrumbs(path);
  el.editorTitle.textContent = `Editor - ${path}`;
  el.editorWelcome.style.display = "none";
  el.editorGrid.style.display = "";

  if (!state.editorView) {
    state.editorView = createEditor(tab.content, tab.path);
  } else {
    setEditorContent(tab.content);
    setModelLanguageForPath(activeModel(), tab.path);
  }
  state.editorView.updateOptions({
    readOnly: !state.writeEnabled || Boolean(tab.readOnly),
  });

  if (!preserveSelection) {
    const model = activeModel();
    const firstColumn = model ? model.getLineFirstNonWhitespaceColumn(1) || 1 : 1;
    const position = new monaco.Position(1, firstColumn);
    state.editorView.setPosition(position);
    state.editorView.revealPositionInCenter(position);
  }

  state.editorView.focus();
  renderFileTree();
  renderTabs();
  syncSecondaryEditor();
  updateCursorLabel();
  scheduleCursorInsights(cursorPosition());
  updateDraftInfo();
  updateSaveBadge(tab.dirty ? "warn" : "ok", tab.dirty ? "dirty" : "saved");
  scheduleDiagnostics({immediate: true});
  setStatus(`Active file: ${path}`);
  postPresenceEvent(path);
  refreshMultiTabCollision();
}

// ── Tasks (build/test/validate) ────────────────────────

function normalizeTaskLocationPath(path) {
  const raw = String(path || "").trim().replaceAll("\\", "/");
  if (!raw) {
    return "";
  }
  return raw.startsWith("./") ? raw.slice(2) : raw;
}

function setRetryAction(action, message) {
  state.lastFailedAction = action || null;
  el.retryActionBtn.disabled = !state.lastFailedAction;
  if (message) {
    setStatus(message);
  }
}

async function retryLastFailedAction() {
  const action = state.lastFailedAction;
  if (!action) {
    setStatus("No failed action to retry.");
    return;
  }
  if (action.kind === "save") {
    if (action.path && state.activePath !== action.path && state.openTabs.has(action.path)) {
      await switchTab(action.path, {preserveSelection: true});
    }
    await saveActiveTab({explicit: true});
    return;
  }
  if (action.kind === "build" || action.kind === "test" || action.kind === "validate") {
    await startTask(action.kind);
    return;
  }
  setStatus(`Unsupported retry action: ${action.kind}`);
}

function renderTaskLinks(locations) {
  el.taskLinksPanel.innerHTML = "";
  if (!Array.isArray(locations) || locations.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = "No source links detected.";
    el.taskLinksPanel.appendChild(empty);
    return;
  }
  for (const location of locations.slice(0, 60)) {
    const path = normalizeTaskLocationPath(location.path);
    const line = Math.max(1, Number(location.line || 1));
    const column = Math.max(1, Number(location.column || 1));
    const button = document.createElement("button");
    button.type = "button";
    button.className = "ide-link-button";
    button.textContent = `${path}:${line}:${column} ${location.message || ""}`.trim();
    button.disabled = !path.toLowerCase().endsWith(".st");
    button.addEventListener("click", async () => {
      if (!path || !path.toLowerCase().endsWith(".st")) {
        return;
      }
      await openFile(path);
      jumpToRange({
        start: {
          line: Math.max(0, line - 1),
          character: Math.max(0, column - 1),
        },
      });
    });
    el.taskLinksPanel.appendChild(button);
  }
}

function renderTaskOutput(task) {
  if (!task) {
    el.taskStatus.textContent = "No task running.";
    el.taskOutput.textContent = "Build/Test/Validate output will appear here.";
    renderTaskLinks([]);
    return;
  }
  const status = task.status || "running";
  const suffix = task.success === true ? "success" : task.success === false ? "failed" : "running";
  const started = formatTimestampMs(task.started_ms);
  const finished = task.finished_ms ? formatTimestampMs(task.finished_ms) : null;
  const timing = finished
    ? `started ${started}, finished ${finished}`
    : `started ${started}`;
  el.taskStatus.textContent = `${task.kind} #${task.job_id}: ${status} (${suffix}) | ${timing}`;
  el.taskOutput.textContent = task.output || "";
  renderTaskLinks(task.locations || []);
}

function stopTaskPolling() {
  if (state.taskPollTimer) {
    clearInterval(state.taskPollTimer);
    state.taskPollTimer = null;
  }
}

async function pollActiveTask() {
  if (!state.activeTaskId) {
    return;
  }
  const task = await apiJson(`/api/ide/task?id=${state.activeTaskId}`, {
    method: "GET",
    headers: apiHeaders(),
    timeoutMs: 3000,
  });
  renderTaskOutput(task);
  const done = task.status === "completed";
  if (done) {
    stopTaskPolling();
    if (task.success) {
      setRetryAction(null, `Task ${task.kind} finished (ok).`);
    } else {
      setRetryAction({kind: task.kind}, `Task ${task.kind} finished (failed). Retry is available.`);
    }
  }
}

async function startTask(kind) {
  try {
    await flushDirtyTabs();
    const endpoint = kind === "build"
      ? "/api/ide/build"
      : kind === "validate"
        ? "/api/ide/validate"
        : "/api/ide/test";
    const task = await apiJson(endpoint, {
      method: "POST",
      headers: apiHeaders(),
      body: "{}",
      timeoutMs: 3000,
    });
    state.activeTaskId = task.job_id;
    renderTaskOutput(task);
    stopTaskPolling();
    state.taskPollTimer = setInterval(() => {
      pollActiveTask().catch(() => {});
    }, 700);
    setRetryAction(null, `Task started: ${kind} #${task.job_id}`);
  } catch (error) {
    const message = String(error?.message || error);
    setRetryAction({kind}, `Task ${kind} failed to start: ${message}`);
    throw error;
  }
}

// ── Search & Symbols ───────────────────────────────────

async function gotoDefinitionAtCursor() {
  const tab = activeTab();
  const position = cursorPosition();
  if (!tab || !position) {
    return;
  }
  if (!isStructuredTextPath(tab.path)) {
    setStatus("Go to definition is available for .st files.");
    return;
  }
  syncDocumentsToWasm();
  if (!wasmClient) {
    setStatus("WASM analysis not available.");
    return;
  }
  const result = await wasmClient.definition(tab.path, position);
  if (!result || !result.uri) {
    setStatus("Definition not found.");
    return;
  }
  await openFile(result.uri);
  jumpToRange(result.range);
  setStatus(`Definition: ${result.uri}`);
}

async function refreshReferencesAtPosition(position, {quiet = false} = {}) {
  const tab = activeTab();
  if (!tab || !position || !isStructuredTextPath(tab.path)) {
    state.references = [];
    renderReferences(state.references);
    return [];
  }
  syncDocumentsToWasm();
  if (!wasmClient) {
    state.references = [];
    renderReferences(state.references);
    return [];
  }
  const result = await wasmClient.references(tab.path, position, true);
  state.references = Array.isArray(result) ? result : [];
  renderReferences(state.references);
  if (!quiet) {
    setStatus(`References: ${state.references.length}`);
  }
  return state.references;
}

function scheduleCursorInsights(position) {
  if (cursorInsightTimer) {
    clearTimeout(cursorInsightTimer);
    cursorInsightTimer = null;
  }
  cursorInsightTimer = setTimeout(() => {
    refreshReferencesAtPosition(position, {quiet: true}).catch((error) => {
      console.warn("[ide] reference refresh failed:", error);
    });
  }, 260);
}

async function findReferencesAtCursor() {
  const position = cursorPosition();
  if (!position) {
    return;
  }
  const tab = activeTab();
  if (!tab || !isStructuredTextPath(tab.path)) {
    setStatus("Find references is available for .st files.");
    return;
  }
  await refreshReferencesAtPosition(position, {quiet: false});
}

async function renameSymbolAtCursor() {
  const tab = activeTab();
  const position = cursorPosition();
  if (!tab || !position) {
    return;
  }
  if (!isStructuredTextPath(tab.path)) {
    setStatus("Rename symbol is available for .st files.");
    return;
  }
  const newName = await idePrompt("Rename symbol to:");
  if (!newName || !newName.trim()) {
    return;
  }
  syncDocumentsToWasm();
  if (!wasmClient) {
    setStatus("WASM analysis not available.");
    return;
  }
  const edits = await wasmClient.rename(tab.path, position, newName.trim());
  if (!Array.isArray(edits) || edits.length === 0) {
    setStatus("Rename produced no edits.");
    return;
  }
  // Group edits by uri and apply them to open tabs
  const editsByUri = new Map();
  for (const edit of edits) {
    if (!edit.uri) {
      continue;
    }
    if (!editsByUri.has(edit.uri)) {
      editsByUri.set(edit.uri, []);
    }
    editsByUri.get(edit.uri).push(edit);
  }
  const changedUris = new Set();
  for (const [uri, fileEdits] of editsByUri) {
    const existing = state.openTabs.get(uri);
    if (!existing) {
      continue;
    }
    // Apply edits in reverse order to preserve offsets
    const sorted = fileEdits.sort((a, b) => {
      const lineDiff = (b.range.start.line || 0) - (a.range.start.line || 0);
      if (lineDiff !== 0) return lineDiff;
      return (b.range.start.character || 0) - (a.range.start.character || 0);
    });
    let content = existing.content;
    for (const edit of sorted) {
      const startOffset = positionToContentOffset(content, edit.range.start);
      const endOffset = positionToContentOffset(content, edit.range.end);
      if (startOffset !== null && endOffset !== null) {
        content = content.slice(0, startOffset) + edit.new_text + content.slice(endOffset);
      }
    }
    existing.content = content;
    existing.dirty = existing.content !== existing.savedContent;
    changedUris.add(uri);
  }
  renderTabs();
  if (state.activePath && state.openTabs.has(state.activePath)) {
    await switchTab(state.activePath, {preserveSelection: true});
  }
  syncDocumentsToWasm();
  scheduleDiagnostics();
  setStatus(`Rename applied across ${changedUris.size} file(s).`);
}

async function workspaceSearchFlow() {
  const query = await idePrompt("Workspace search query:");
  if (!query || !query.trim()) {
    return;
  }
  const include = await idePrompt("Include glob (optional, e.g. **/*.st):", "**/*.st");
  const exclude = await idePrompt("Exclude glob (optional):", "");
  const params = new URLSearchParams({
    q: query.trim(),
    limit: "120",
  });
  if (include && include.trim()) {
    params.set("include", include.trim());
  }
  if (exclude && exclude.trim()) {
    params.set("exclude", exclude.trim());
  }
  const result = await apiJson(`/api/ide/search?${params.toString()}`, {
    method: "GET",
    headers: apiHeaders(),
  });
  state.searchHits = Array.isArray(result) ? result : [];
  renderSearchHits(state.searchHits);
  setStatus(`Search results: ${state.searchHits.length}`);
}

async function fetchSymbols(query = "", path = "") {
  const scoped = path ? `&path=${encodeURIComponent(path)}` : "";
  const result = await apiJson(`/api/ide/symbols?q=${encodeURIComponent(query)}&limit=120${scoped}`, {
    method: "GET",
    headers: apiHeaders(),
  });
  return Array.isArray(result) ? result : [];
}

async function fileSymbolSearchFlow() {
  const path = activeTab()?.path;
  if (!path) {
    setStatus("Open a file first to search file symbols.");
    return;
  }
  if (!isStructuredTextPath(path)) {
    setStatus("File symbol search is available for .st files.");
    return;
  }
  const query = await idePrompt(`File symbol query (${path}):`, "");
  if (query === null) {
    return;
  }
  const symbols = await fetchSymbols(query.trim(), path);
  renderSymbolHits(symbols);
  setStatus(`File symbols: ${symbols.length}`);
}

async function workspaceSymbolSearchFlow() {
  const query = await idePrompt("Workspace symbol query:", "");
  if (query === null) {
    return;
  }
  const symbols = await fetchSymbols(query.trim(), "");
  renderSymbolHits(symbols);
  setStatus(`Workspace symbols: ${symbols.length}`);
}

async function loadFsAuditLog() {
  const events = await apiJson("/api/ide/fs/audit?limit=80", {
    method: "GET",
    headers: apiHeaders(),
  });
  el.searchPanel.innerHTML = "";
  if (!Array.isArray(events) || events.length === 0) {
    const empty = document.createElement("div");
    empty.className = "muted";
    empty.textContent = "No filesystem mutation events.";
    el.searchPanel.appendChild(empty);
    return;
  }
  for (const event of events) {
    const row = document.createElement("div");
    row.className = "muted";
    row.textContent = `${event.ts_secs || 0}  ${event.action || "event"}  ${event.path || ""}`;
    el.searchPanel.appendChild(row);
  }
  setStatus(`Filesystem audit events: ${events.length}`);
}

// ── Diagnostics ────────────────────────────────────────

function resetAnalysisFailureState() {
  state.analysis.consecutiveFailures = 0;
  if (state.analysis.degraded) {
    state.analysis.degraded = false;
    updateLatencyBadge();
    setStatus("Analysis recovered.");
  }
}

function noteAnalysisFailure(error, source) {
  const message = String(error?.message || error);
  if (isTimeoutMessage(message)) {
    bumpTelemetry("analysis_timeouts");
  }
  state.analysis.consecutiveFailures += 1;
  if (state.analysis.consecutiveFailures < 3) {
    return;
  }

  const now = Date.now();
  const firstDegrade = !state.analysis.degraded;
  state.analysis.degraded = true;
  updateLatencyBadge();
  if (firstDegrade || now - state.analysis.lastNoticeAtMs > 4_000) {
    const suffix = source === "completion"
      ? "Completion may be delayed while analysis retries."
      : "IDE is retrying analysis requests.";
    setStatus(`Analysis degraded after repeated failures. ${suffix}`);
    state.analysis.lastNoticeAtMs = now;
  }
}

async function fetchDiagnostics(docText) {
  const tab = activeTab();
  if (!tab) {
    return [];
  }
  if (!isStructuredTextPath(tab.path)) {
    return [];
  }

  if (docText.length > 180_000) {
    return [];
  }

  const started = performance.now();
  try {
    syncDocumentsToWasm();
    if (!wasmClient) {
      return [];
    }
    const result = await wasmClient.diagnostics(tab.path);
    resetAnalysisFailureState();
    const elapsed = performance.now() - started;
    state.latencySamples.push(elapsed);
    if (state.latencySamples.length > 40) {
      state.latencySamples.shift();
    }
    updateLatencyBadge();
    return Array.isArray(result) ? result : [];
  } catch (error) {
    noteAnalysisFailure(error, "diagnostics");
    throw error;
  }
}

async function fetchHover(position) {
  const tab = activeTab();
  if (!tab) {
    return null;
  }
  if (!isStructuredTextPath(tab.path)) {
    return null;
  }
  try {
    syncDocumentsToWasm();
    if (!wasmClient) {
      return null;
    }
    const result = await wasmClient.hover(tab.path, position);
    resetAnalysisFailureState();
    return result;
  } catch (error) {
    noteAnalysisFailure(error, "hover");
    throw error;
  }
}

async function fetchCompletion(position, limit = 40) {
  const tab = activeTab();
  if (!tab) {
    return [];
  }
  if (!isStructuredTextPath(tab.path)) {
    return [];
  }
  try {
    syncDocumentsToWasm();
    if (!wasmClient) {
      return [];
    }
    const result = await wasmClient.completion(tab.path, position, limit);
    resetAnalysisFailureState();
    return Array.isArray(result) ? result : [];
  } catch (error) {
    noteAnalysisFailure(error, "completion");
    console.warn("[ide] completion request failed:", error);
    return [];
  }
}

async function runDiagnosticsForActiveEditor() {
  const tab = activeTab();
  const model = activeModel();
  if (!tab || !model || !state.editorView) {
    return;
  }
  if (!isStructuredTextPath(tab.path)) {
    state.diagnostics = [];
    diagnosticsToProblems([]);
    applyMonacoMarkers([], model);
    return;
  }

  const ticket = ++state.diagnosticsTicket;
  const text = state.editorView.getValue();
  try {
    const diagnostics = await fetchDiagnostics(text);
    if (ticket !== state.diagnosticsTicket) {
      return;
    }
    state.diagnostics = diagnostics;
    diagnosticsToProblems(diagnostics);
    applyMonacoMarkers(diagnostics, model);
  } catch (error) {
    if (ticket !== state.diagnosticsTicket) {
      return;
    }
    setStatus(`Diagnostics request failed: ${String(error.message || error)}`);
    state.diagnostics = [];
    diagnosticsToProblems([]);
    applyMonacoMarkers([], model);
  }
}

function scheduleDiagnostics({immediate = false} = {}) {
  if (state.diagnosticsTimer) {
    clearTimeout(state.diagnosticsTimer);
    state.diagnosticsTimer = null;
  }
  if (!state.editorView || !activeTab()) {
    return;
  }
  if (immediate) {
    runDiagnosticsForActiveEditor().catch(() => {});
    return;
  }
  state.diagnosticsTimer = setTimeout(() => {
    runDiagnosticsForActiveEditor().catch(() => {});
  }, 220);
}

// ── Health & Telemetry ─────────────────────────────────

function renderHealthPanel(payload) {
  const info = payload || {};
  const healthy = !state.analysis.degraded;
  const sessions = info.active_sessions ?? 0;
  const sorted = [...state.latencySamples].sort((a, b) => a - b);
  const p95Index = Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95));
  const p95 = sorted.length > 0 ? `${Math.round(sorted[p95Index])}ms` : "--";

  el.healthPanel.innerHTML = "";
  const statusRow = document.createElement("div");
  statusRow.className = "row";
  const badge = healthy ? "ok" : "warn";
  statusRow.innerHTML = `<span class="ide-badge ${badge}">${healthy ? "Healthy" : "Degraded"}</span>`;
  el.healthPanel.appendChild(statusRow);

  const rows = [
    ["Sessions", sessions],
    ["Diag p95", p95],
  ];
  for (const [label, value] of rows) {
    const row = document.createElement("div");
    row.className = "row";
    row.innerHTML = `<span class="muted">${label}</span> <span class="stat">${value}</span>`;
    el.healthPanel.appendChild(row);
  }
}

async function pollHealth() {
  if (!state.sessionToken) {
    return;
  }
  try {
    const health = await apiJson("/api/ide/health", {
      method: "GET",
      headers: apiHeaders(),
    });
    renderHealthPanel(health);
  } catch {
    renderHealthPanel(null);
  }
}

function scheduleHealthPoll() {
  if (state.healthTimer) {
    clearInterval(state.healthTimer);
  }
  state.healthTimer = setInterval(() => {
    pollHealth().catch(() => {});
  }, 5000);
}

async function flushFrontendTelemetry() {
  if (!state.sessionToken) {
    return;
  }
  try {
    await apiJson("/api/ide/frontend-telemetry", {
      method: "POST",
      headers: apiHeaders(),
      body: JSON.stringify(state.telemetry),
      timeoutMs: 2500,
      allowSessionRetry: true,
    });
  } catch {
    // Keep counters locally; they'll be retried on next flush.
  }
}

function scheduleTelemetryFlush() {
  if (state.telemetryTimer) {
    clearInterval(state.telemetryTimer);
  }
  state.telemetryTimer = setInterval(() => {
    flushFrontendTelemetry().catch(() => {});
  }, 7000);
}

// ── Presence / Collaboration ───────────────────────────

function postPresenceEvent(path) {
  if (!path) {
    return;
  }
  const payload = {
    type: "active-file",
    tab_id: state.tabId,
    path,
    ts: Date.now(),
  };
  try {
    if (state.presenceChannel) {
      state.presenceChannel.postMessage(payload);
    }
  } catch {
    // no-op
  }
  try {
    localStorage.setItem(IDE_PRESENCE_STORAGE_KEY, JSON.stringify(payload));
  } catch {
    // no-op
  }
}

function consumePresencePayload(payload) {
  if (!payload || payload.type !== "active-file") {
    return;
  }
  if (payload.tab_id === state.tabId) {
    return;
  }
  if (!payload.path || typeof payload.path !== "string") {
    return;
  }
  state.peerClaims.set(payload.path, {
    tabId: payload.tab_id,
    ts: Number(payload.ts || Date.now()),
  });
  refreshMultiTabCollision();
}

function refreshMultiTabCollision() {
  const now = Date.now();
  for (const [path, claim] of state.peerClaims.entries()) {
    if (!claim || now - claim.ts > IDE_PRESENCE_CLAIM_TTL_MS) {
      state.peerClaims.delete(path);
    }
  }
  const active = state.activePath;
  if (!active) {
    state.collisionPath = null;
    return;
  }
  const claim = state.peerClaims.get(active);
  if (claim) {
    state.collisionPath = active;
    updateSaveBadge("warn", "multi-tab");
    setStatus(`Multi-tab warning: ${active} is open in another browser tab.`);
    return;
  }
  state.collisionPath = null;
}

// Presence model stub
async function loadPresenceModel() {
  // UI badge removed in phase-2 polish; presence data not shown.
}

// ── Folder Browser ─────────────────────────────────────

function hideBrowseListing() {
  if (el.browseListing) {
    el.browseListing.style.display = "none";
  }
  state.browseVisible = false;
}

async function browseTo(dirPath) {
  try {
    const params = dirPath ? `?path=${encodeURIComponent(dirPath)}` : "";
    const result = await apiJson(`/api/ide/browse${params}`, {
      method: "GET",
      headers: apiHeaders(),
      timeoutMs: 3000,
    });
    renderBrowseEntries(result);
    if (el.browseListing) {
      el.browseListing.style.display = "";
    }
    state.browseVisible = true;
  } catch (error) {
    setStatus(`Browse failed: ${error.message || error}`);
  }
}

function renderBrowseEntries(data) {
  if (!el.browseEntries || !el.browseBreadcrumbs) {
    return;
  }
  const currentPath = data.current_path || "/";
  const parentPath = data.parent_path || null;
  const entries = Array.isArray(data.entries) ? data.entries : [];

  el.browseBreadcrumbs.textContent = currentPath;

  el.browseEntries.innerHTML = "";
  if (parentPath !== null) {
    const up = document.createElement("button");
    up.type = "button";
    up.className = "ide-browse-entry directory";
    up.innerHTML = `<svg viewBox="0 0 16 16"><path d="M8 12V4M4 8l4-4 4 4"/></svg>`;
    const label = document.createElement("span");
    label.textContent = "..";
    up.appendChild(label);
    up.addEventListener("click", () => browseTo(parentPath));
    el.browseEntries.appendChild(up);
  }

  for (const entry of entries) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = `ide-browse-entry${entry.kind === "directory" ? " directory" : ""}`;
    if (entry.kind === "directory") {
      row.innerHTML = `<svg viewBox="0 0 16 16"><path d="M2 13V4a1 1 0 0 1 1-1h3.5l2 2H13a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z"/></svg>`;
    } else {
      row.innerHTML = `<svg viewBox="0 0 16 16"><path d="M4 2h5l4 4v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z"/><path d="M9 2v4h4"/></svg>`;
    }
    const label = document.createElement("span");
    label.textContent = entry.name;
    row.appendChild(label);

    row.addEventListener("click", () => {
      el.openProjectInput.value = entry.path;
    });
    row.addEventListener("dblclick", () => {
      if (entry.kind === "directory") {
        browseTo(entry.path);
      }
    });
    el.browseEntries.appendChild(row);
  }
}

// ── Modals ─────────────────────────────────────────────

function idePrompt(title, defaultValue = "") {
  return new Promise((resolve) => {
    el.inputModalTitle.textContent = title;
    el.inputModalField.value = defaultValue;
    el.inputModal.classList.add("open");
    el.inputModalField.focus();
    el.inputModalField.select();

    const cleanup = () => {
      el.inputModal.classList.remove("open");
      el.inputModalOk.removeEventListener("click", onOk);
      el.inputModalCancel.removeEventListener("click", onCancel);
      el.inputModalField.removeEventListener("keydown", onKey);
      el.inputModal.removeEventListener("click", onBackdrop);
    };
    const onOk = () => { cleanup(); resolve(el.inputModalField.value); };
    const onCancel = () => { cleanup(); resolve(null); };
    const onKey = (e) => {
      if (e.key === "Enter") { e.preventDefault(); onOk(); }
      if (e.key === "Escape") { e.preventDefault(); onCancel(); }
    };
    const onBackdrop = (e) => { if (e.target === el.inputModal) onCancel(); };
    el.inputModalOk.addEventListener("click", onOk);
    el.inputModalCancel.addEventListener("click", onCancel);
    el.inputModalField.addEventListener("keydown", onKey);
    el.inputModal.addEventListener("click", onBackdrop);
  });
}

function ideConfirm(title, message) {
  return new Promise((resolve) => {
    el.confirmModalTitle.textContent = title;
    el.confirmModalMessage.textContent = message;
    el.confirmModal.classList.add("open");
    el.confirmModalOk.focus();

    const cleanup = () => {
      el.confirmModal.classList.remove("open");
      el.confirmModalOk.removeEventListener("click", onOk);
      el.confirmModalCancel.removeEventListener("click", onCancel);
      el.confirmModal.removeEventListener("click", onBackdrop);
      document.removeEventListener("keydown", onKey);
    };
    const onOk = () => { cleanup(); resolve(true); };
    const onCancel = () => { cleanup(); resolve(false); };
    const onKey = (e) => {
      if (e.key === "Enter") { e.preventDefault(); onOk(); }
      if (e.key === "Escape") { e.preventDefault(); onCancel(); }
    };
    const onBackdrop = (e) => { if (e.target === el.confirmModal) onCancel(); };
    el.confirmModalOk.addEventListener("click", onOk);
    el.confirmModalCancel.addEventListener("click", onCancel);
    el.confirmModal.addEventListener("click", onBackdrop);
    document.addEventListener("keydown", onKey);
  });
}

// ── Command Palette ────────────────────────────────────

function nextTab() {
  const paths = [...state.openTabs.keys()];
  if (paths.length <= 1 || !state.activePath) {
    return;
  }
  const index = paths.indexOf(state.activePath);
  const next = paths[(index + 1) % paths.length];
  switchTab(next).catch(() => {});
}

function previousTab() {
  const paths = [...state.openTabs.keys()];
  if (paths.length <= 1 || !state.activePath) {
    return;
  }
  const index = paths.indexOf(state.activePath);
  const prev = paths[(index - 1 + paths.length) % paths.length];
  switchTab(prev).catch(() => {});
}

function closePalette() {
  el.commandPalette.classList.remove("open");
  el.commandInput.value = "";
  state.commandFilter = "";
}

function openQuickOpenPalette() {
  if (state.files.length === 0) {
    setStatus("No files available. Open a project folder first.");
    return;
  }
  state.commands = state.files.map((path) => ({
    id: `open:${path}`,
    label: `Open ${path}`,
    run: () => openFile(path),
  }));
  state.commandFilter = "";
  state.selectedCommandIndex = 0;
  renderCommandList();
  el.commandPalette.classList.add("open");
  el.commandInput.focus();
}

function paletteCommands() {
  return [
    {id: "save", label: "Save active file", run: () => saveActiveTab({explicit: true})},
    {id: "save-all", label: "Save all open files", run: () => flushDirtyTabs()},
    {id: "open-project", label: "Open project folder", run: () => openProjectFlow()},
    {id: "format-document", label: "Format document", run: () => formatActiveDocument()},
    {id: "quick-open", label: "Quick open file", run: () => openQuickOpenPalette()},
    {id: "file-symbols", label: "File symbols", run: () => fileSymbolSearchFlow()},
    {id: "workspace-symbols", label: "Workspace symbols", run: () => workspaceSymbolSearchFlow()},
    {id: "goto-definition", label: "Go to definition", run: () => gotoDefinitionAtCursor()},
    {id: "find-references", label: "Find references", run: () => findReferencesAtCursor()},
    {id: "rename-symbol", label: "Rename symbol", run: () => renameSymbolAtCursor()},
    {id: "workspace-search", label: "Workspace search", run: () => workspaceSearchFlow()},
    {id: "fs-audit", label: "Filesystem audit log", run: () => loadFsAuditLog()},
    {id: "validate", label: "Validate project", run: () => startTask("validate")},
    {id: "build", label: "Build project", run: () => startTask("build")},
    {id: "test", label: "Run project tests", run: () => startTask("test")},
    {id: "retry-last", label: "Retry last failed action", run: () => retryLastFailedAction()},
    {id: "toggle-split", label: "Toggle split editor", run: () => toggleSplitEditor()},
    {id: "theme", label: "Toggle dark/light mode", run: () => toggleTheme()},
    {id: "next-tab", label: "Next tab", run: () => nextTab()},
    {id: "prev-tab", label: "Previous tab", run: () => previousTab()},
    {id: "refresh-files", label: "Refresh file tree", run: () => bootstrapFiles()},
    {
      id: "recover-analysis",
      label: "Recover analysis mode",
      run: () => {
        state.analysis.degraded = false;
        state.analysis.consecutiveFailures = 0;
        state.analysis.lastNoticeAtMs = 0;
        updateLatencyBadge();
        setStatus("Analysis mode reset.");
      },
    },
    {id: "a11y", label: "Show accessibility baseline path", run: () => setStatus(`Accessibility baseline: ${A11Y_REPORT_LINK}`)},
    {id: "completion", label: "Trigger completion", run: () => state.editorView && startCompletion(state.editorView)},
  ];
}

function renderCommandList() {
  const filter = state.commandFilter.trim().toLowerCase();
  const commands = state.commands.filter((cmd) => {
    if (!filter) return true;
    return cmd.label.toLowerCase().includes(filter);
  });
  if (commands.length === 0) {
    el.commandList.innerHTML = "<div class='muted'>No matching commands.</div>";
    return;
  }
  state.selectedCommandIndex = Math.min(state.selectedCommandIndex, commands.length - 1);
  el.commandList.innerHTML = "";
  commands.forEach((command, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `ide-command${index === state.selectedCommandIndex ? " active" : ""}`;
    button.textContent = command.label;
    button.addEventListener("mouseenter", () => {
      state.selectedCommandIndex = index;
      renderCommandList();
    });
    button.addEventListener("click", async () => {
      closePalette();
      await command.run();
    });
    el.commandList.appendChild(button);
  });
}

function openCommandPalette() {
  state.commands = paletteCommands();
  state.commandFilter = "";
  state.selectedCommandIndex = 0;
  renderCommandList();
  el.commandPalette.classList.add("open");
  el.commandInput.focus();
}

async function runSelectedCommand() {
  const filter = state.commandFilter.trim().toLowerCase();
  const commands = state.commands.filter((cmd) => {
    if (!filter) return true;
    return cmd.label.toLowerCase().includes(filter);
  });
  if (commands.length === 0) {
    return;
  }
  const selected = commands[state.selectedCommandIndex] || commands[0];
  closePalette();
  await selected.run();
}

// ── Event Binding ──────────────────────────────────────

function bindGlobalEvents() {
  bindResizeHandles();

  // DRY: action bindings
  bindAction(el.saveBtn, () => saveActiveTab({explicit: true}));
  bindAction(el.saveAllBtn, () => flushDirtyTabs());
  bindAction(el.buildBtn, () => startTask("build"), "Build failed");
  bindAction(el.validateBtn, () => startTask("validate"), "Validate failed");
  bindAction(el.testBtn, () => startTask("test"), "Test failed");
  bindAction(el.retryActionBtn, () => retryLastFailedAction(), "Retry failed");
  el.splitBtn.addEventListener("click", () => toggleSplitEditor());
  el.editorPanePrimary.addEventListener("mousedown", () => { if (state.splitEnabled) setActivePane("primary"); });
  el.editorPaneSecondary.addEventListener("mousedown", () => { if (state.splitEnabled) setActivePane("secondary"); });
  bindAction(el.openProjectBtn, () => openProjectFlow(), "Open folder failed");
  el.quickOpenBtn.addEventListener("click", () => openQuickOpenPalette());
  bindAction(el.welcomeOpenBtn, () => openProjectFlow(), "Open folder failed");
  el.welcomeQuickOpenBtn.addEventListener("click", () => openQuickOpenPalette());
  bindAction(el.newFileBtn, () => createPath("file"), "Create file failed");
  bindAction(el.newFolderBtn, () => createPath("directory"), "Create folder failed");
  bindAction(el.renamePathBtn, () => renameSelectedPath(), "Rename failed");
  bindAction(el.deletePathBtn, () => deleteSelectedPath(), "Delete failed");

  el.fileFilterInput.addEventListener("input", (event) => {
    state.fileFilter = String(event.target.value || "").trim().toLowerCase();
    renderFileTree();
  });
  el.themeToggle.addEventListener("click", () => toggleTheme());
  el.cmdPaletteBtn.addEventListener("click", () => openCommandPalette());

  // Context menu actions
  el.ctxOpenBtn.addEventListener("click", () => {
    const path = state.contextPath;
    closeTreeContextMenu();
    if (!path) return;
    if (nodeKindForPath(path) === "file") {
      openFile(path).catch((error) => setStatus(`Open failed: ${error.message || error}`));
    } else {
      toggleDir(path);
    }
  });
  bindAction(el.ctxNewFileBtn, () => { closeTreeContextMenu(); return createPath("file"); }, "Create file failed");
  bindAction(el.ctxNewFolderBtn, () => { closeTreeContextMenu(); return createPath("directory"); }, "Create folder failed");
  bindAction(el.ctxRenameBtn, () => { closeTreeContextMenu(); return renameSelectedPath(); }, "Rename failed");
  bindAction(el.ctxDeleteBtn, () => { closeTreeContextMenu(); return deleteSelectedPath(); }, "Delete failed");

  // Open project panel
  el.openProjectOk.addEventListener("click", () => {
    const val = el.openProjectInput.value;
    closeOpenProjectPanel();
    doOpenProject(val).catch((error) => setStatus(`Open folder failed: ${error.message || error}`));
  });
  el.openProjectCancel.addEventListener("click", () => closeOpenProjectPanel());

  // Browse button
  if (el.browseBtn) {
    el.browseBtn.addEventListener("click", () => {
      if (state.browseVisible) {
        hideBrowseListing();
      } else {
        const current = el.openProjectInput.value.trim() || undefined;
        browseTo(current);
      }
    });
  }

  el.openProjectInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      const items = state._recentItems || [];
      const idx = state._recentSelectedIndex ?? -1;
      if (idx >= 0 && idx < items.length) {
        items[idx].click();
      } else {
        const val = el.openProjectInput.value;
        closeOpenProjectPanel();
        doOpenProject(val).catch((error) => setStatus(`Open folder failed: ${error.message || error}`));
      }
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeOpenProjectPanel();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      const items = state._recentItems || [];
      if (items.length > 0) {
        const idx = (state._recentSelectedIndex ?? -1) + 1;
        state._recentSelectedIndex = idx >= items.length ? 0 : idx;
        items.forEach((r, i) => r.classList.toggle("active", i === state._recentSelectedIndex));
      }
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      const items = state._recentItems || [];
      if (items.length > 0) {
        const idx = (state._recentSelectedIndex ?? 0) - 1;
        state._recentSelectedIndex = idx < 0 ? items.length - 1 : idx;
        items.forEach((r, i) => r.classList.toggle("active", i === state._recentSelectedIndex));
      }
    }
  });

  for (const header of document.querySelectorAll(".ide-section-header")) {
    header.addEventListener("click", () => {
      const section = header.closest(".ide-section");
      if (!section) return;
      const collapsed = section.classList.toggle("collapsed");
      header.setAttribute("aria-expanded", String(!collapsed));
    });
  }

  if (typeof BroadcastChannel !== "undefined") {
    try {
      state.presenceChannel = new BroadcastChannel(IDE_PRESENCE_CHANNEL);
      state.presenceChannel.onmessage = (event) => {
        consumePresencePayload(event.data);
      };
    } catch {
      state.presenceChannel = null;
    }
  }

  el.commandInput.addEventListener("input", (event) => {
    state.commandFilter = event.target.value || "";
    state.selectedCommandIndex = 0;
    renderCommandList();
  });

  el.commandInput.addEventListener("keydown", (event) => {
    const filter = state.commandFilter.trim().toLowerCase();
    const commands = state.commands.filter((cmd) => {
      if (!filter) return true;
      return cmd.label.toLowerCase().includes(filter);
    });
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (commands.length > 0) {
        state.selectedCommandIndex = (state.selectedCommandIndex + 1) % commands.length;
        renderCommandList();
      }
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (commands.length > 0) {
        state.selectedCommandIndex = (state.selectedCommandIndex - 1 + commands.length) % commands.length;
        renderCommandList();
      }
    } else if (event.key === "Enter") {
      event.preventDefault();
      runSelectedCommand().catch(() => {});
    } else if (event.key === "Escape") {
      event.preventDefault();
      closePalette();
    }
  });

  el.commandPalette.addEventListener("click", (event) => {
    if (event.target === el.commandPalette) {
      closePalette();
    }
  });

  document.addEventListener("click", (event) => {
    if (!el.treeContextMenu.classList.contains("ide-hidden")) {
      const target = event.target;
      if (target instanceof Node && !el.treeContextMenu.contains(target)) {
        closeTreeContextMenu();
      }
    }
  });

  window.addEventListener("online", () => {
    state.online = true;
    updateConnectionBadge();
    setStatus("Connection restored. Flushing dirty drafts...");
    flushDirtyTabs().catch(() => {});
    flushFrontendTelemetry().catch(() => {});
  });

  window.addEventListener("offline", () => {
    state.online = false;
    updateConnectionBadge();
    updateSaveBadge("err", "offline draft");
    setStatus("Connection lost. Drafts are stored locally.");
  });

  window.addEventListener("storage", (event) => {
    if (event.key !== IDE_PRESENCE_STORAGE_KEY || !event.newValue) {
      return;
    }
    try {
      consumePresencePayload(JSON.parse(event.newValue));
    } catch {
      // no-op
    }
  });

  window.addEventListener("keydown", (event) => {
    const isMod = event.ctrlKey || event.metaKey;
    if (isMod && event.shiftKey && event.key.toLowerCase() === "p") {
      event.preventDefault();
      openCommandPalette();
      return;
    }
    if (isMod && event.shiftKey && event.key.toLowerCase() === "o") {
      event.preventDefault();
      fileSymbolSearchFlow().catch((error) => setStatus(`File symbols failed: ${error.message || error}`));
      return;
    }
    if (isMod && event.shiftKey && event.key.toLowerCase() === "f") {
      event.preventDefault();
      workspaceSearchFlow().catch((error) => setStatus(`Search failed: ${error.message || error}`));
      return;
    }
    if (isMod && !event.shiftKey && event.key.toLowerCase() === "p") {
      event.preventDefault();
      openQuickOpenPalette();
      return;
    }
    if (event.key === "F1") {
      event.preventDefault();
      openCommandPalette();
      return;
    }
    if (event.shiftKey && event.altKey && event.key.toLowerCase() === "f") {
      event.preventDefault();
      formatActiveDocument().catch((error) => setStatus(`Format failed: ${error.message || error}`));
      return;
    }
    if (isMod && !event.shiftKey && event.key.toLowerCase() === "s") {
      event.preventDefault();
      saveActiveTab({explicit: true}).catch(() => {});
      return;
    }
    if (isMod && event.code === "Space") {
      event.preventDefault();
      startCompletion();
      return;
    }
    if (isMod && event.key === "Tab") {
      event.preventDefault();
      if (event.shiftKey) {
        previousTab();
      } else {
        nextTab();
      }
      return;
    }
    if (event.key === "F12" && !event.shiftKey) {
      event.preventDefault();
      gotoDefinitionAtCursor().catch((error) => setStatus(`Definition failed: ${error.message || error}`));
      return;
    }
    if (event.key === "F12" && event.shiftKey) {
      event.preventDefault();
      findReferencesAtCursor().catch((error) => setStatus(`References failed: ${error.message || error}`));
      return;
    }
    if (event.key === "F2") {
      event.preventDefault();
      renameSymbolAtCursor().catch((error) => setStatus(`Rename failed: ${error.message || error}`));
    }
    if (event.key === "Escape" && el.openProjectPanel.classList.contains("open")) {
      closeOpenProjectPanel();
      return;
    }
    if (event.key === "Escape" && el.commandPalette.classList.contains("open")) {
      closePalette();
      return;
    }
    if (event.key === "Escape" && !el.treeContextMenu.classList.contains("ide-hidden")) {
      closeTreeContextMenu();
    }
  });

  window.addEventListener("beforeunload", () => {
    flushFrontendTelemetry().catch(() => {});
    stopTaskPolling();
    disposeEditorDisposables();
    completionProviderDisposable?.dispose();
    hoverProviderDisposable?.dispose();
    if (cursorInsightTimer) {
      clearTimeout(cursorInsightTimer);
      cursorInsightTimer = null;
    }
    if (completionTriggerTimer) {
      clearTimeout(completionTriggerTimer);
      completionTriggerTimer = null;
    }
    if (cursorHoverPopupTimer) {
      clearHoverPopupTimer();
    }
    if (state.editorView) {
      state.editorView.dispose();
    }
    if (state.secondaryEditorView) {
      state.secondaryEditorView.dispose();
    }
    if (state.presenceChannel) {
      state.presenceChannel.close();
    }
  });
}

// ── Bootstrap ──────────────────────────────────────────

async function bootstrapSession() {
  const caps = await apiJson("/api/ide/capabilities");
  state.writeEnabled = caps.mode === "authoring";
  el.statusMode.textContent = state.writeEnabled ? "Authoring" : "Read-only";
  el.newFileBtn.disabled = !state.writeEnabled;
  el.newFolderBtn.disabled = !state.writeEnabled;
  el.renamePathBtn.disabled = !state.writeEnabled;
  el.deletePathBtn.disabled = !state.writeEnabled;
  el.saveBtn.disabled = !state.writeEnabled;
  el.saveAllBtn.disabled = !state.writeEnabled;
  el.validateBtn.disabled = !state.writeEnabled;
  el.buildBtn.disabled = !state.writeEnabled;
  el.testBtn.disabled = !state.writeEnabled;

  const role = state.writeEnabled ? "editor" : "viewer";
  const session = await apiJson("/api/ide/session", {
    method: "POST",
    headers: apiHeaders(),
    body: JSON.stringify({role}),
  });
  state.sessionToken = session.token;
  setStatus(`Session ${session.role} active. ${state.writeEnabled ? "Autosave enabled." : "Read-only mode."}`);
  await refreshProjectSelection();
}

async function bootstrap() {
  updateConnectionBadge();
  applyWorkbenchSizing();
  const storedTheme = localStorage.getItem(THEME_STORAGE_KEY);
  if (!storedTheme) {
    const preferred = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
    applyTheme(preferred);
  } else {
    applyTheme(storedTheme);
  }

  bindGlobalEvents();
  try {
    const modulesLoaded = await loadEditorModules();
    if (!modulesLoaded) {
      bumpTelemetry("bootstrap_failures");
      flushFrontendTelemetry().catch(() => {});
      return;
    }
    await bootstrapSession();
    await loadPresenceModel();
    await bootstrapFiles();
    await initWasmAnalysis();
    syncDocumentsToWasm();
    await pollHealth();
    scheduleHealthPoll();
    scheduleTelemetryFlush();
    renderReferences([]);
    renderSearchHits([]);
    renderTaskOutput(null);
    setRetryAction(null, null);
    el.splitBtn.title = "Split";
    setStatus("IDE ready.");
    updateSaveBadge("ok", state.writeEnabled ? "saved" : "read-only");
    state.ready = true;
  } catch (error) {
    bumpTelemetry("bootstrap_failures");
    setStatus(`IDE bootstrap failed: ${String(error.message || error)}`);
    updateSaveBadge("err", "error");
    setStatus("Failed to initialize IDE session. Check auth/runtime mode.");
    flushFrontendTelemetry().catch(() => {});
  }
}

bootstrap();
