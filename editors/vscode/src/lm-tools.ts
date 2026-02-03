import * as path from "path";
import { TextDecoder } from "util";
import * as vscode from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

type LmApi = {
  registerTool: <T>(
    name: string,
    tool: {
      invoke: (
        options: InvocationOptions<T>,
        token: vscode.CancellationToken,
      ) => Promise<unknown> | unknown;
    },
  ) => vscode.Disposable;
};

type InvocationOptions<T> = {
  input: T;
};

type LspClientProvider = () => LanguageClient | undefined;

type LmToolResultCtor = new (parts: unknown[]) => unknown;
type LmTextPartCtor = new (value: string) => unknown;

const languageModelToolResultCtor = (
  vscode as unknown as { LanguageModelToolResult?: LmToolResultCtor }
).LanguageModelToolResult;

const languageModelTextPartCtor = (
  vscode as unknown as { LanguageModelTextPart?: LmTextPartCtor }
).LanguageModelTextPart;

const MAX_ITEMS = 200;

interface PositionParams {
  filePath: string;
  line: number;
  character: number;
}

interface DiagnosticsParams {
  filePath: string;
}

interface ReferencesParams extends PositionParams {
  includeDeclaration?: boolean;
}

interface CompletionParams extends PositionParams {
  triggerCharacter?: string;
}

interface WorkspaceSymbolsParams {
  query: string;
  limit?: number;
}

interface RenameParams extends PositionParams {
  newName: string;
}

interface RangeParams {
  filePath: string;
  startLine: number;
  startCharacter: number;
  endLine: number;
  endCharacter: number;
}

interface RangePositionsParams {
  filePath: string;
  positions: Array<{ line: number; character: number }>;
}

interface SemanticTokensDeltaParams {
  filePath: string;
  previousResultId: string;
}

interface SemanticTokensRangeParams extends RangeParams {}

interface InlayHintsParams extends RangeParams {}

interface LinkedEditingParams extends PositionParams {}

interface DocumentLinksParams {
  filePath: string;
  resolve?: boolean;
}

interface CodeLensParams {
  filePath: string;
  resolve?: boolean;
}

interface OnTypeFormattingParams extends PositionParams {
  triggerCharacter: string;
}

interface LspRequestParams {
  method: string;
  params?: unknown;
  requestTimeoutMs?: number;
  captureNotifications?: string[];
  notificationTimeoutMs?: number;
  captureProgress?: boolean;
  capturePartialResults?: boolean;
  workDoneToken?: string;
  partialResultToken?: string;
}

interface LspNotificationParams {
  method: string;
  params?: unknown;
}

interface WorkspaceFileRenameParams {
  oldPath: string;
  newPath: string;
  overwrite?: boolean;
  useWorkspaceEdit?: boolean;
}

interface SettingsToggleParams {
  key: string;
  value: unknown;
  scope?: "workspace" | "global" | "workspaceFolder";
  filePath?: string;
  timeoutMs?: number;
  forceRefresh?: boolean;
}

interface TelemetryReadParams {
  filePath?: string;
  limit?: number;
  tail?: boolean;
}

interface WorkspaceSymbolsTimedParams extends WorkspaceSymbolsParams {
  pathIncludes?: string[];
}

interface InlineValuesParams {
  frameId: number;
  startLine: number;
  startCharacter: number;
  endLine: number;
  endCharacter: number;
  context?: Record<string, unknown>;
}

interface ProjectInfoParams {
  arguments?: unknown[];
}

interface FileReadParams {
  filePath: string;
  startLine?: number;
  startCharacter?: number;
  endLine?: number;
  endCharacter?: number;
}

interface FileWriteParams {
  filePath: string;
  text: string;
  save?: boolean;
}

interface ApplyEditsParams {
  filePath: string;
  edits: Array<{
    startLine: number;
    startCharacter: number;
    endLine: number;
    endCharacter: number;
    newText: string;
  }>;
  save?: boolean;
}

interface DebugStartParams {
  filePath?: string;
}

type EmptyParams = Record<string, never>;

function lmAvailable(): boolean {
  const lm = (vscode as unknown as { lm?: LmApi }).lm;
  return !!(lm && languageModelToolResultCtor && languageModelTextPartCtor);
}

function textResult(text: string): unknown {
  if (!languageModelToolResultCtor || !languageModelTextPartCtor) {
    return { text };
  }
  return new languageModelToolResultCtor([new languageModelTextPartCtor(text)]);
}

function errorResult(message: string): unknown {
  return textResult(`Error: ${message}`);
}

function clientUnavailableResult(): unknown {
  return errorResult("Language client is not available.");
}

function uriFromFilePath(filePath: string): vscode.Uri | undefined {
  const trimmed = filePath.trim();
  if (!trimmed) {
    return undefined;
  }
  if (trimmed.startsWith("file://")) {
    try {
      return vscode.Uri.parse(trimmed);
    } catch {
      return undefined;
    }
  }
  const hasScheme = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(trimmed);
  const isWindowsPath = /^[a-zA-Z]:[\\/]/.test(trimmed);
  if (hasScheme && !isWindowsPath) {
    try {
      return vscode.Uri.parse(trimmed);
    } catch {
      return undefined;
    }
  }
  if (!path.isAbsolute(trimmed)) {
    return undefined;
  }
  return vscode.Uri.file(trimmed);
}

function optionalUriFromFilePath(
  filePath: string | undefined,
): vscode.Uri | undefined {
  if (!filePath) {
    return undefined;
  }
  const trimmed = filePath.trim();
  if (!trimmed) {
    return undefined;
  }
  return uriFromFilePath(trimmed);
}

function isPathInside(base: string, target: string): boolean {
  const rel = path.relative(base, target);
  return rel === "" || (!rel.startsWith("..") && !path.isAbsolute(rel));
}

function ensureWorkspaceUri(uri: vscode.Uri): string | undefined {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return "No workspace is open.";
  }
  const normalizedTarget = path.normalize(uri.fsPath);
  for (const folder of folders) {
    const normalizedBase = path.normalize(folder.uri.fsPath);
    if (isPathInside(normalizedBase, normalizedTarget)) {
      return undefined;
    }
  }
  return "filePath must be inside the current workspace.";
}

async function ensureDocument(uri: vscode.Uri): Promise<vscode.TextDocument> {
  const open = openDocumentIfLoaded(uri);
  if (open) {
    return open;
  }
  return vscode.workspace.openTextDocument(uri);
}

function openDocumentIfLoaded(
  uri: vscode.Uri,
): vscode.TextDocument | undefined {
  return vscode.workspace.textDocuments.find(
    (doc) => doc.uri.toString() === uri.toString(),
  );
}

function fullDocumentRange(doc: vscode.TextDocument): vscode.Range {
  if (doc.lineCount === 0) {
    return new vscode.Range(
      new vscode.Position(0, 0),
      new vscode.Position(0, 0),
    );
  }
  const lastLine = doc.lineCount - 1;
  return new vscode.Range(
    new vscode.Position(0, 0),
    doc.lineAt(lastLine).range.end,
  );
}

function resolveRange(
  doc: vscode.TextDocument,
  startLine: number,
  startCharacter: number,
  endLine: number,
  endCharacter: number,
): vscode.Range {
  const start = resolvePosition(doc, startLine, startCharacter);
  const end = resolvePosition(doc, endLine, endCharacter);
  return start.isBefore(end)
    ? new vscode.Range(start, end)
    : new vscode.Range(end, start);
}

function resolvePosition(
  doc: vscode.TextDocument,
  line: number,
  character: number,
): vscode.Position {
  const safeLine = Math.max(0, Math.min(line, doc.lineCount - 1));
  const safeChar = Math.max(
    0,
    Math.min(character, doc.lineAt(safeLine).text.length),
  );
  return new vscode.Position(safeLine, safeChar);
}

async function waitForDiagnostics(
  uri: vscode.Uri,
  token: vscode.CancellationToken,
  timeoutMs = 1000,
): Promise<boolean> {
  if (token.isCancellationRequested) {
    return false;
  }
  return new Promise<boolean>((resolve) => {
    let settled = false;
    const finish = (value: boolean) => {
      if (settled) {
        return;
      }
      settled = true;
      disposable.dispose();
      resolve(value);
    };
    const timer = setTimeout(() => finish(false), timeoutMs);
    const disposable = vscode.languages.onDidChangeDiagnostics((event) => {
      if (event.uris.some((changed) => changed.toString() === uri.toString())) {
        clearTimeout(timer);
        finish(true);
      }
    });
    token.onCancellationRequested(() => {
      clearTimeout(timer);
      finish(false);
    });
  });
}

function resolveLspPosition(position: vscode.Position): {
  line: number;
  character: number;
} {
  return { line: position.line, character: position.character };
}

function resolveLspRange(range: vscode.Range): {
  start: { line: number; character: number };
  end: { line: number; character: number };
} {
  return { start: resolveLspPosition(range.start), end: resolveLspPosition(range.end) };
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  timeoutMessage: string,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(timeoutMessage)), timeoutMs);
    promise
      .then((value) => {
        clearTimeout(timer);
        resolve(value);
      })
      .catch((err) => {
        clearTimeout(timer);
        reject(err);
      });
  });
}

function makeProgressToken(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
}

async function sendLspRequest(
  getClient: LspClientProvider | undefined,
  method: string,
  params: unknown,
  token: vscode.CancellationToken,
  options?: {
    requestTimeoutMs?: number;
    captureNotifications?: string[];
    notificationTimeoutMs?: number;
  },
): Promise<
  | { response: unknown; notifications: Array<{ method: string; params: unknown }> }
  | { error: string }
> {
  if (token.isCancellationRequested) {
    return { error: "Cancelled." };
  }
  const client = getClient?.();
  if (!client) {
    return { error: "Language client is not available." };
  }
  const notifications: Array<{ method: string; params: unknown }> = [];
  const disposables: vscode.Disposable[] = [];
  if (options?.captureNotifications) {
    for (const name of options.captureNotifications) {
      disposables.push(
        client.onNotification(name, (payload) => {
          notifications.push({ method: name, params: payload });
        }),
      );
    }
  }
  try {
    const requestPromise = client.sendRequest(method, params);
    const response =
      typeof options?.requestTimeoutMs === "number"
        ? await withTimeout(
            requestPromise,
            options.requestTimeoutMs,
            `LSP request timed out after ${options.requestTimeoutMs}ms.`,
          )
        : await requestPromise;
    if (options?.notificationTimeoutMs) {
      await new Promise<void>((resolve) =>
        setTimeout(resolve, options.notificationTimeoutMs),
      );
    }
    return { response, notifications };
  } catch (error) {
    return { error: String(error) };
  } finally {
    for (const disposable of disposables) {
      disposable.dispose();
    }
  }
}

function diagnosticsPayload(diagnostics: vscode.Diagnostic[]): {
  diagnostics: Array<{
    severity: string;
    message: string;
    range: string;
    source?: string;
    code?: string | number;
  }>;
} {
  return {
    diagnostics: diagnostics.map((diag) => ({
      severity: vscode.DiagnosticSeverity[diag.severity],
      message: diag.message,
      range: formatRange(diag.range),
      source: diag.source ?? undefined,
      code:
        typeof diag.code === "string" || typeof diag.code === "number"
          ? diag.code
          : diag.code?.value,
    })),
  };
}

function toLspDiagnostic(diagnostic: vscode.Diagnostic): {
  range: { start: { line: number; character: number }; end: { line: number; character: number } };
  severity?: number;
  code?: string | number;
  source?: string;
  message: string;
  relatedInformation?: Array<{
    location: { uri: string; range: { start: { line: number; character: number }; end: { line: number; character: number } } };
    message: string;
  }>;
  tags?: number[];
} {
  const code =
    typeof diagnostic.code === "string" || typeof diagnostic.code === "number"
      ? diagnostic.code
      : diagnostic.code?.value;
  const relatedInformation = diagnostic.relatedInformation?.map((info) => ({
    location: {
      uri: info.location.uri.toString(),
      range: resolveLspRange(info.location.range),
    },
    message: info.message,
  }));
  return {
    range: resolveLspRange(diagnostic.range),
    severity: diagnostic.severity,
    code: code ?? undefined,
    source: diagnostic.source ?? undefined,
    message: diagnostic.message,
    relatedInformation: relatedInformation?.length ? relatedInformation : undefined,
    tags: diagnostic.tags?.length ? diagnostic.tags : undefined,
  };
}

function diagnosticsForRange(
  uri: vscode.Uri,
  range: vscode.Range,
): vscode.Diagnostic[] {
  return vscode
    .languages
    .getDiagnostics(uri)
    .filter((diag) => !!diag.range.intersection(range));
}

function renderMarkup(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (value && typeof value === "object") {
    const asRecord = value as Record<string, unknown>;
    if (typeof asRecord.value === "string") {
      return asRecord.value;
    }
    if (
      typeof asRecord.language === "string" &&
      typeof asRecord.value === "string"
    ) {
      return asRecord.value;
    }
  }
  return value ? String(value) : "";
}

function formatRange(range: vscode.Range): string {
  return `${range.start.line + 1}:${range.start.character + 1}-${
    range.end.line + 1
  }:${range.end.character + 1}`;
}

function formatUriString(uri: string): string {
  try {
    return vscode.Uri.parse(uri).fsPath;
  } catch {
    return uri;
  }
}

function formatLspRange(range: {
  start: { line: number; character: number };
  end: { line: number; character: number };
}): string {
  return `${range.start.line + 1}:${range.start.character + 1}-${
    range.end.line + 1
  }:${range.end.character + 1}`;
}

function formatLspLocation(location: { uri: string; range: any }): string {
  return `${formatUriString(location.uri)}:${location.range.start.line + 1}:${
    location.range.start.character + 1
  }`;
}

function formatLocationLike(
  location: vscode.Location | vscode.LocationLink,
): string {
  const uri = "uri" in location ? location.uri : location.targetUri;
  const range = "range" in location ? location.range : location.targetRange;
  return `${uri.fsPath}:${range.start.line + 1}:${range.start.character + 1}`;
}

function truncateItems<T>(
  items: T[],
  limit = MAX_ITEMS,
): { items: T[]; truncated: boolean } {
  if (items.length <= limit) {
    return { items, truncated: false };
  }
  return { items: items.slice(0, limit), truncated: true };
}

function completionLabel(label: vscode.CompletionItem["label"]): string {
  return typeof label === "string" ? label : label.label;
}

function completionDocumentation(
  documentation: vscode.CompletionItem["documentation"],
): string | undefined {
  if (!documentation) {
    return undefined;
  }
  if (typeof documentation === "string") {
    return documentation;
  }
  return renderMarkup(documentation);
}

function completionInsertText(
  insertText: vscode.CompletionItem["insertText"],
): string | undefined {
  if (!insertText) {
    return undefined;
  }
  if (typeof insertText === "string") {
    return insertText;
  }
  return insertText.value;
}

function symbolKindName(
  kind: vscode.SymbolKind | undefined,
): string | undefined {
  if (typeof kind !== "number") {
    return undefined;
  }
  return vscode.SymbolKind[kind];
}

function completionKindName(
  kind: vscode.CompletionItemKind | undefined,
): string | undefined {
  if (typeof kind !== "number") {
    return undefined;
  }
  return vscode.CompletionItemKind[kind];
}

type InlayHintLabelValue = string | vscode.InlayHintLabelPart[];

function inlayHintLabel(label: InlayHintLabelValue): string {
  if (typeof label === "string") {
    return label;
  }
  return label.map((part: vscode.InlayHintLabelPart) => part.value).join("");
}

function documentSymbolsToList(
  symbols: vscode.DocumentSymbol[],
  prefix = "",
): Array<{
  name: string;
  kind?: string;
  detail?: string;
  range: string;
  selectionRange: string;
  path: string;
}> {
  const items: Array<{
    name: string;
    kind?: string;
    detail?: string;
    range: string;
    selectionRange: string;
    path: string;
  }> = [];
  for (const symbol of symbols) {
    const pathSegment = prefix ? `${prefix}.${symbol.name}` : symbol.name;
    items.push({
      name: symbol.name,
      kind: symbolKindName(symbol.kind),
      detail: symbol.detail || undefined,
      range: formatRange(symbol.range),
      selectionRange: formatRange(symbol.selectionRange),
      path: pathSegment,
    });
    if (symbol.children && symbol.children.length > 0) {
      items.push(...documentSymbolsToList(symbol.children, pathSegment));
    }
  }
  return items;
}

function workspaceEditEntries(
  edit: vscode.WorkspaceEdit,
): Array<{ uri: vscode.Uri; edits: vscode.TextEdit[] }> {
  const anyEdit = edit as unknown as {
    entries?: () => Array<[vscode.Uri, vscode.TextEdit[]]>;
    changes?: Record<string, vscode.TextEdit[]>;
    documentChanges?: Array<any>;
  };
  if (typeof anyEdit.entries === "function") {
    return anyEdit.entries().map(([uri, edits]) => ({ uri, edits }));
  }
  if (anyEdit.changes) {
    return Object.entries(anyEdit.changes).map(([uri, edits]) => ({
      uri: vscode.Uri.parse(uri),
      edits,
    }));
  }
  if (Array.isArray(anyEdit.documentChanges)) {
    const entries: Array<{ uri: vscode.Uri; edits: vscode.TextEdit[] }> = [];
    for (const change of anyEdit.documentChanges) {
      if ("edits" in change && "textDocument" in change) {
        entries.push({ uri: change.textDocument.uri, edits: change.edits });
      }
    }
    return entries;
  }
  return [];
}

function summarizeWorkspaceEdit(edit: vscode.WorkspaceEdit): {
  files: Array<{
    filePath: string;
    edits: Array<{
      range: string;
      newTextPreview: string;
    }>;
  }>;
  truncated: boolean;
} {
  const entries = workspaceEditEntries(edit);
  const flattened: Array<{
    filePath: string;
    range: string;
    newText: string;
  }> = [];
  for (const entry of entries) {
    for (const textEdit of entry.edits) {
      flattened.push({
        filePath: entry.uri.fsPath,
        range: formatRange(textEdit.range),
        newText: textEdit.newText,
      });
    }
  }
  const { items, truncated } = truncateItems(flattened);
  const grouped = new Map<
    string,
    Array<{ range: string; newTextPreview: string }>
  >();
  for (const item of items) {
    const preview =
      item.newText.length > 120
        ? `${item.newText.slice(0, 117)}...`
        : item.newText;
    const edits = grouped.get(item.filePath) ?? [];
    edits.push({ range: item.range, newTextPreview: preview });
    grouped.set(item.filePath, edits);
  }
  return {
    files: Array.from(grouped.entries()).map(([filePath, edits]) => ({
      filePath,
      edits,
    })),
    truncated,
  };
}

function summarizeLspTextEdits(
  edits: Array<{ range: { start: any; end: any }; newText: string }>,
): { edits: Array<{ range: string; newTextPreview: string }>; truncated: boolean } {
  const { items, truncated } = truncateItems(edits);
  const summarized = items.map((edit) => ({
    range: formatLspRange(edit.range),
    newTextPreview:
      edit.newText.length > 120
        ? `${edit.newText.slice(0, 117)}...`
        : edit.newText,
  }));
  return { edits: summarized, truncated };
}

function summarizeSemanticTokens(result: unknown): unknown {
  if (!result || typeof result !== "object") {
    return result;
  }
  const record = result as {
    resultId?: string;
    data?: number[];
    edits?: Array<{ start: number; deleteCount: number; data?: number[] }>;
  };
  if (Array.isArray(record.data)) {
    return {
      resultId: record.resultId ?? undefined,
      dataLength: record.data.length,
      data: record.data,
    };
  }
  if (Array.isArray(record.edits)) {
    return {
      resultId: record.resultId ?? undefined,
      edits: record.edits.map((edit) => ({
        start: edit.start,
        deleteCount: edit.deleteCount,
        dataLength: Array.isArray(edit.data) ? edit.data.length : 0,
        data: Array.isArray(edit.data) ? edit.data : [],
      })),
    };
  }
  return result;
}

export class STHoverTool {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        uri,
        position,
      );
      if (!hovers || hovers.length === 0) {
        return textResult("No hover information available at this position.");
      }
      const content = hovers
        .flatMap((hover) => hover.contents)
        .map((item) => renderMarkup(item))
        .filter((item) => item.length > 0)
        .join("\n\n");
      return textResult(
        content || "No hover information available at this position.",
      );
    } catch (error) {
      return errorResult(`Failed to get hover info: ${String(error)}`);
    }
  }
}

export class STDiagnosticsTool {
  async invoke(
    options: InvocationOptions<DiagnosticsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const alreadyOpen = vscode.workspace.textDocuments.some(
      (doc) => doc.uri.toString() === uri.toString(),
    );
    try {
      await ensureDocument(uri);
      if (!alreadyOpen) {
        await waitForDiagnostics(uri, token);
      }
      const diagnostics = vscode.languages.getDiagnostics(uri);
      if (diagnostics.length === 0) {
        return textResult("No diagnostics (errors or warnings) found.");
      }
      return textResult(
        JSON.stringify(diagnosticsPayload(diagnostics), null, 2),
      );
    } catch (error) {
      return errorResult(`Failed to get diagnostics: ${String(error)}`);
    }
  }
}

export class STDefinitionTool {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const definitions = await vscode.commands.executeCommand<
        vscode.Location[] | vscode.LocationLink[]
      >("vscode.executeDefinitionProvider", uri, position);
      if (!definitions || definitions.length === 0) {
        return textResult("No definition found.");
      }
      const locations = definitions.map(formatLocationLike);
      return textResult(JSON.stringify({ locations }, null, 2));
    } catch (error) {
      return errorResult(`Failed to find definition: ${String(error)}`);
    }
  }
}

export class STDeclarationTool {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const locations = await vscode.commands.executeCommand<
        vscode.Location[] | vscode.LocationLink[]
      >("vscode.executeDeclarationProvider", uri, position);
      if (!locations || locations.length === 0) {
        return textResult("No declaration found.");
      }
      const formatted = locations.map(formatLocationLike);
      return textResult(JSON.stringify({ locations: formatted }, null, 2));
    } catch (error) {
      return errorResult(`Failed to find declaration: ${String(error)}`);
    }
  }
}

export class STTypeDefinitionTool {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const locations = await vscode.commands.executeCommand<
        vscode.Location[] | vscode.LocationLink[]
      >("vscode.executeTypeDefinitionProvider", uri, position);
      if (!locations || locations.length === 0) {
        return textResult("No type definition found.");
      }
      const formatted = locations.map(formatLocationLike);
      return textResult(JSON.stringify({ locations: formatted }, null, 2));
    } catch (error) {
      return errorResult(`Failed to find type definition: ${String(error)}`);
    }
  }
}

export class STImplementationTool {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const implementations = await vscode.commands.executeCommand<
        vscode.Location[] | vscode.LocationLink[]
      >("vscode.executeImplementationProvider", uri, position);
      if (!implementations || implementations.length === 0) {
        return textResult("No implementations found.");
      }
      const locations = implementations.map(formatLocationLike);
      return textResult(JSON.stringify({ locations }, null, 2));
    } catch (error) {
      return errorResult(`Failed to find implementations: ${String(error)}`);
    }
  }
}

export class STReferencesTool {
  async invoke(
    options: InvocationOptions<ReferencesParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character, includeDeclaration } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const references = await vscode.commands.executeCommand<
        vscode.Location[]
      >("vscode.executeReferenceProvider", uri, position, {
        includeDeclaration: includeDeclaration ?? true,
      });
      if (!references || references.length === 0) {
        return textResult("No references found.");
      }
      const locations = references.map((ref) => formatLocationLike(ref));
      return textResult(JSON.stringify({ locations }, null, 2));
    } catch (error) {
      return errorResult(`Failed to find references: ${String(error)}`);
    }
  }
}

export class STCompletionTool {
  async invoke(
    options: InvocationOptions<CompletionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character, triggerCharacter } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const completions = await vscode.commands.executeCommand<
        vscode.CompletionList | vscode.CompletionItem[]
      >(
        "vscode.executeCompletionItemProvider",
        uri,
        position,
        triggerCharacter,
      );
      if (!completions) {
        return textResult("No completion items returned.");
      }
      const items = Array.isArray(completions)
        ? completions
        : completions.items;
      const { items: trimmed, truncated } = truncateItems(items);
      const payload = trimmed.map((item) => ({
        label: completionLabel(item.label),
        kind: completionKindName(item.kind),
        detail: item.detail || undefined,
        documentation: completionDocumentation(item.documentation),
        insertText: completionInsertText(item.insertText),
      }));
      return textResult(JSON.stringify({ items: payload, truncated }, null, 2));
    } catch (error) {
      return errorResult(`Failed to get completions: ${String(error)}`);
    }
  }
}

export class STSignatureHelpTool {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const signatureHelp =
        await vscode.commands.executeCommand<vscode.SignatureHelp>(
          "vscode.executeSignatureHelpProvider",
          uri,
          position,
        );
      if (!signatureHelp || signatureHelp.signatures.length === 0) {
        return textResult("No signature help available.");
      }
      const payload = signatureHelp.signatures.map((sig, index) => ({
        label: sig.label,
        documentation: sig.documentation
          ? renderMarkup(sig.documentation as unknown)
          : undefined,
        parameters: sig.parameters?.map((param) => ({
          label: param.label,
          documentation: param.documentation
            ? renderMarkup(param.documentation as unknown)
            : undefined,
        })),
        isActiveSignature: index === signatureHelp.activeSignature,
      }));
      return textResult(
        JSON.stringify(
          {
            activeSignature: signatureHelp.activeSignature ?? 0,
            activeParameter: signatureHelp.activeParameter ?? 0,
            signatures: payload,
          },
          null,
          2,
        ),
      );
    } catch (error) {
      return errorResult(`Failed to get signature help: ${String(error)}`);
    }
  }
}

export class STDocumentSymbolsTool {
  async invoke(
    options: InvocationOptions<DiagnosticsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      await ensureDocument(uri);
      const symbols = await vscode.commands.executeCommand<
        vscode.DocumentSymbol[] | vscode.SymbolInformation[]
      >("vscode.executeDocumentSymbolProvider", uri);
      if (!symbols || symbols.length === 0) {
        return textResult("No document symbols found.");
      }
      if (symbols.length > 0 && "location" in symbols[0]) {
        const infoSymbols = symbols as vscode.SymbolInformation[];
        const { items, truncated } = truncateItems(infoSymbols);
        const payload = items.map((symbol) => ({
          name: symbol.name,
          kind: symbolKindName(symbol.kind),
          containerName: symbol.containerName || undefined,
          location: formatLocationLike(symbol.location),
        }));
        return textResult(
          JSON.stringify({ symbols: payload, truncated }, null, 2),
        );
      }
      const docSymbols = symbols as vscode.DocumentSymbol[];
      const flattened = documentSymbolsToList(docSymbols);
      const { items, truncated } = truncateItems(flattened);
      return textResult(JSON.stringify({ symbols: items, truncated }, null, 2));
    } catch (error) {
      return errorResult(`Failed to get document symbols: ${String(error)}`);
    }
  }
}

export class STWorkspaceSymbolsTool {
  async invoke(
    options: InvocationOptions<WorkspaceSymbolsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { query, limit } = options.input;
    if (!query.trim()) {
      return errorResult("query must be a non-empty string.");
    }
    try {
      const symbols = await vscode.commands.executeCommand<
        vscode.SymbolInformation[]
      >("vscode.executeWorkspaceSymbolProvider", query);
      if (!symbols || symbols.length === 0) {
        return textResult("No workspace symbols found.");
      }
      const { items, truncated } = truncateItems(symbols, limit ?? MAX_ITEMS);
      const payload = items.map((symbol) => ({
        name: symbol.name,
        kind: symbolKindName(symbol.kind),
        containerName: symbol.containerName || undefined,
        location: formatLocationLike(symbol.location),
      }));
      return textResult(
        JSON.stringify({ symbols: payload, truncated }, null, 2),
      );
    } catch (error) {
      return errorResult(`Failed to get workspace symbols: ${String(error)}`);
    }
  }
}

export class STRenameTool {
  async invoke(
    options: InvocationOptions<RenameParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character, newName } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    if (!newName.trim()) {
      return errorResult("newName must be a non-empty string.");
    }
    try {
      const doc = await ensureDocument(uri);
      const position = resolvePosition(doc, line, character);
      const edit = await vscode.commands.executeCommand<vscode.WorkspaceEdit>(
        "vscode.executeDocumentRenameProvider",
        uri,
        position,
        newName,
      );
      if (!edit) {
        return textResult("No rename edits returned.");
      }
      const summary = summarizeWorkspaceEdit(edit);
      return textResult(JSON.stringify({ edit: summary }, null, 2));
    } catch (error) {
      return errorResult(`Failed to rename symbol: ${String(error)}`);
    }
  }
}

export class STFormatTool {
  async invoke(
    options: InvocationOptions<DiagnosticsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      await ensureDocument(uri);
      const editorConfig = vscode.workspace.getConfiguration("editor", uri);
      const formattingOptions: vscode.FormattingOptions = {
        insertSpaces: editorConfig.get<boolean>("insertSpaces", true),
        tabSize: editorConfig.get<number>("tabSize", 2),
      };
      const edits = await vscode.commands.executeCommand<vscode.TextEdit[]>(
        "vscode.executeFormatDocumentProvider",
        uri,
        formattingOptions,
      );
      if (!edits || edits.length === 0) {
        return textResult("No formatting edits returned.");
      }
      const edit = summarizeWorkspaceEdit({
        set: () => {},
        insert: () => {},
        delete: () => {},
        replace: () => {},
        entries: () => [[uri, edits]],
        size: edits.length,
        has: () => true,
      } as unknown as vscode.WorkspaceEdit);
      return textResult(JSON.stringify({ edit }, null, 2));
    } catch (error) {
      return errorResult(`Failed to format document: ${String(error)}`);
    }
  }
}

class LspToolBase {
  constructor(protected readonly getClient?: LspClientProvider) {}

  protected async request(
    method: string,
    params: unknown,
    token: vscode.CancellationToken,
    options?: {
      requestTimeoutMs?: number;
      captureNotifications?: string[];
      notificationTimeoutMs?: number;
    },
  ): Promise<
    | { response: unknown; notifications: Array<{ method: string; params: unknown }> }
    | { error: string }
  > {
    return sendLspRequest(this.getClient, method, params, token, options);
  }
}

export class STCodeActionsTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<RangeParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, startLine, startCharacter, endLine, endCharacter } =
      options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    try {
      const doc = await ensureDocument(uri);
      const range = resolveRange(
        doc,
        startLine,
        startCharacter,
        endLine,
        endCharacter,
      );
      const diagnostics = diagnosticsForRange(uri, range).map(toLspDiagnostic);
      const params = {
        textDocument: { uri: uri.toString() },
        range: resolveLspRange(range),
        context: {
          diagnostics,
          triggerKind: 1,
        },
      };
      const result = await this.request("textDocument/codeAction", params, token);
      if ("error" in result) {
        return errorResult(result.error);
      }
      const actions = Array.isArray(result.response) ? result.response : [];
      if (actions.length === 0) {
        return textResult("No code actions found.");
      }
      const payload = actions.map((action) => {
        const isCodeAction =
          action &&
          typeof action === "object" &&
          ("edit" in action ||
            "kind" in action ||
            "isPreferred" in action ||
            "disabled" in action);
        if (!isCodeAction) {
          const cmd = action as vscode.Command;
          return {
            title: cmd.title,
            command: cmd.command,
            arguments: cmd.arguments,
          };
        }
        const codeAction = action as vscode.CodeAction;
        const kind =
          typeof codeAction.kind === "string"
            ? codeAction.kind
            : codeAction.kind?.value;
        return {
          title: codeAction.title,
          kind,
          isPreferred: codeAction.isPreferred ?? false,
          command: codeAction.command?.command,
          arguments: codeAction.command?.arguments,
        };
      });
      return textResult(JSON.stringify({ actions: payload }, null, 2));
    } catch (error) {
      return errorResult(`Failed to get code actions: ${String(error)}`);
    }
  }
}

export class STLspRequestTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<LspRequestParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const {
      method,
      params,
      requestTimeoutMs,
      captureNotifications,
      notificationTimeoutMs,
      captureProgress,
      capturePartialResults,
      workDoneToken,
      partialResultToken,
    } = options.input;
    if (!method || !method.trim()) {
      return errorResult("method must be a non-empty string.");
    }
    const notificationSet = new Set(captureNotifications ?? []);
    let nextParams = params;
    let usedWorkDoneToken = workDoneToken;
    let usedPartialResultToken = partialResultToken;
    const wantsProgress =
      captureProgress ||
      capturePartialResults ||
      !!workDoneToken ||
      !!partialResultToken;
    if (wantsProgress) {
      if (!params || typeof params !== "object" || Array.isArray(params)) {
        return errorResult(
          "params must be an object when progress/partial tokens are requested.",
        );
      }
      const paramRecord = { ...(params as Record<string, unknown>) };
      if (typeof paramRecord.workDoneToken === "string") {
        usedWorkDoneToken = paramRecord.workDoneToken;
      }
      if (typeof paramRecord.partialResultToken === "string") {
        usedPartialResultToken = paramRecord.partialResultToken;
      }
      if (!usedWorkDoneToken && captureProgress) {
        usedWorkDoneToken = makeProgressToken("trustlsp-work");
      }
      if (!usedPartialResultToken && capturePartialResults) {
        usedPartialResultToken = makeProgressToken("trustlsp-partial");
      }
      if (usedWorkDoneToken && paramRecord.workDoneToken === undefined) {
        paramRecord.workDoneToken = usedWorkDoneToken;
      }
      if (usedPartialResultToken && paramRecord.partialResultToken === undefined) {
        paramRecord.partialResultToken = usedPartialResultToken;
      }
      notificationSet.add("$/progress");
      nextParams = paramRecord;
    }
    const result = await this.request(method, nextParams, token, {
      requestTimeoutMs,
      captureNotifications: Array.from(notificationSet),
      notificationTimeoutMs,
    });
    if ("error" in result) {
      return errorResult(result.error);
    }
    return textResult(
      JSON.stringify(
        {
          response: result.response,
          notifications: result.notifications,
          workDoneToken: usedWorkDoneToken ?? undefined,
          partialResultToken: usedPartialResultToken ?? undefined,
        },
        null,
        2,
      ),
    );
  }
}

export class STLspNotificationTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<LspNotificationParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { method, params } = options.input;
    if (!method || !method.trim()) {
      return errorResult("method must be a non-empty string.");
    }
    const client = this.getClient?.();
    if (!client) {
      return clientUnavailableResult();
    }
    try {
      await client.sendNotification(method, params);
      return textResult("Notification sent.");
    } catch (error) {
      return errorResult(`Failed to send notification: ${String(error)}`);
    }
  }
}

export class STSemanticTokensFullTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<DiagnosticsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const uri = uriFromFilePath(options.input.filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    await ensureDocument(uri);
    const params = { textDocument: { uri: uri.toString() } };
    const result = await this.request(
      "textDocument/semanticTokens/full",
      params,
      token,
    );
    if ("error" in result) {
      return errorResult(result.error);
    }
    return textResult(JSON.stringify(summarizeSemanticTokens(result.response), null, 2));
  }
}

export class STSemanticTokensDeltaTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<SemanticTokensDeltaParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, previousResultId } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    if (!previousResultId.trim()) {
      return errorResult("previousResultId must be a non-empty string.");
    }
    await ensureDocument(uri);
    const params = {
      textDocument: { uri: uri.toString() },
      previousResultId,
    };
    const result = await this.request(
      "textDocument/semanticTokens/full/delta",
      params,
      token,
    );
    if ("error" in result) {
      return errorResult(result.error);
    }
    return textResult(JSON.stringify(summarizeSemanticTokens(result.response), null, 2));
  }
}

export class STSemanticTokensRangeTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<SemanticTokensRangeParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, startLine, startCharacter, endLine, endCharacter } =
      options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const doc = await ensureDocument(uri);
    const range = resolveRange(
      doc,
      startLine,
      startCharacter,
      endLine,
      endCharacter,
    );
    const params = {
      textDocument: { uri: uri.toString() },
      range: resolveLspRange(range),
    };
    const result = await this.request(
      "textDocument/semanticTokens/range",
      params,
      token,
    );
    if ("error" in result) {
      return errorResult(result.error);
    }
    return textResult(JSON.stringify(summarizeSemanticTokens(result.response), null, 2));
  }
}

export class STInlayHintsTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<InlayHintsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, startLine, startCharacter, endLine, endCharacter } =
      options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const doc = await ensureDocument(uri);
    const range = resolveRange(
      doc,
      startLine,
      startCharacter,
      endLine,
      endCharacter,
    );
    const params = { textDocument: { uri: uri.toString() }, range: resolveLspRange(range) };
    const result = await this.request("textDocument/inlayHint", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const hints = Array.isArray(result.response) ? result.response : [];
    const { items, truncated } = truncateItems(hints);
    const payload = items.map((hint) => ({
      position: hint.position
        ? `${hint.position.line + 1}:${hint.position.character + 1}`
        : undefined,
      label: hint.label ? inlayHintLabel(hint.label) : "",
      kind:
        typeof hint.kind === "number" ? vscode.InlayHintKind[hint.kind] : undefined,
      tooltip: hint.tooltip ? renderMarkup(hint.tooltip) : undefined,
      paddingLeft: hint.paddingLeft ?? false,
      paddingRight: hint.paddingRight ?? false,
    }));
    return textResult(JSON.stringify({ hints: payload, truncated }, null, 2));
  }
}

export class STLinkedEditingTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<LinkedEditingParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const doc = await ensureDocument(uri);
    const position = resolvePosition(doc, line, character);
    const params = {
      textDocument: { uri: uri.toString() },
      position: resolveLspPosition(position),
    };
    const result = await this.request("textDocument/linkedEditingRange", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const linked = result.response as { ranges?: any[]; wordPattern?: string } | null;
    if (!linked || !Array.isArray(linked.ranges)) {
      return textResult("No linked editing ranges returned.");
    }
    const ranges = linked.ranges.map((range) => formatLspRange(range));
    return textResult(
      JSON.stringify({ ranges, wordPattern: linked.wordPattern }, null, 2),
    );
  }
}

export class STDocumentLinksTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<DocumentLinksParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, resolve } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    await ensureDocument(uri);
    const params = { textDocument: { uri: uri.toString() } };
    const result = await this.request("textDocument/documentLink", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const links = Array.isArray(result.response) ? result.response : [];
    const { items, truncated } = truncateItems(links);
    const payload: Array<{
      range: string;
      target?: string;
      tooltip?: string;
    }> = [];
    for (const link of items) {
      let resolvedLink = link;
      if (resolve) {
        const resolved = await this.request("documentLink/resolve", link, token);
        if (!("error" in resolved)) {
          resolvedLink = resolved.response;
        }
      }
      payload.push({
        range: resolvedLink.range ? formatLspRange(resolvedLink.range) : "",
        target: resolvedLink.target ? formatUriString(resolvedLink.target) : undefined,
        tooltip: resolvedLink.tooltip ? renderMarkup(resolvedLink.tooltip) : undefined,
      });
    }
    return textResult(JSON.stringify({ links: payload, truncated }, null, 2));
  }
}

export class STCodeLensTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<CodeLensParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, resolve } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    await ensureDocument(uri);
    const params = { textDocument: { uri: uri.toString() } };
    const result = await this.request("textDocument/codeLens", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const lenses = Array.isArray(result.response) ? result.response : [];
    const { items, truncated } = truncateItems(lenses);
    const payload = [];
    for (const lens of items) {
      let resolvedLens = lens;
      if (resolve) {
        const resolved = await this.request("codeLens/resolve", lens, token);
        if (!("error" in resolved)) {
          resolvedLens = resolved.response;
        }
      }
      payload.push({
        range: resolvedLens.range ? formatLspRange(resolvedLens.range) : "",
        command: resolvedLens.command?.title ?? undefined,
      });
    }
    return textResult(JSON.stringify({ lenses: payload, truncated }, null, 2));
  }
}

export class STSelectionRangeTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<RangePositionsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, positions } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    if (!Array.isArray(positions) || positions.length === 0) {
      return errorResult("positions must be a non-empty array.");
    }
    const doc = await ensureDocument(uri);
    const lspPositions = positions.map((pos) =>
      resolveLspPosition(resolvePosition(doc, pos.line, pos.character)),
    );
    const params = { textDocument: { uri: uri.toString() }, positions: lspPositions };
    const result = await this.request("textDocument/selectionRange", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const ranges = Array.isArray(result.response) ? result.response : [];
    const payload = ranges.map((range) => {
      const chain: string[] = [];
      let current = range;
      while (current) {
        chain.push(formatLspRange(current.range));
        current = current.parent;
      }
      return chain;
    });
    return textResult(JSON.stringify({ ranges: payload }, null, 2));
  }
}

export class STOnTypeFormattingTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<OnTypeFormattingParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character, triggerCharacter } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    if (!triggerCharacter) {
      return errorResult("triggerCharacter must be provided.");
    }
    const doc = await ensureDocument(uri);
    const position = resolvePosition(doc, line, character);
    const editorConfig = vscode.workspace.getConfiguration("editor", uri);
    const formattingOptions = {
      insertSpaces: editorConfig.get<boolean>("insertSpaces", true),
      tabSize: editorConfig.get<number>("tabSize", 2),
    };
    const params = {
      textDocument: { uri: uri.toString() },
      position: resolveLspPosition(position),
      ch: triggerCharacter,
      options: formattingOptions,
    };
    const result = await this.request("textDocument/onTypeFormatting", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const edits = Array.isArray(result.response) ? result.response : [];
    const summary = summarizeLspTextEdits(edits);
    return textResult(JSON.stringify(summary, null, 2));
  }
}

export class STCallHierarchyPrepareTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const doc = await ensureDocument(uri);
    const position = resolvePosition(doc, line, character);
    const params = {
      textDocument: { uri: uri.toString() },
      position: resolveLspPosition(position),
    };
    const result = await this.request("textDocument/prepareCallHierarchy", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const items = Array.isArray(result.response) ? result.response : [];
    const payload = items.map((item) => ({
      name: item.name,
      kind: item.kind ? vscode.SymbolKind[item.kind] : undefined,
      uri: formatUriString(item.uri),
      range: item.range ? formatLspRange(item.range) : undefined,
      selectionRange: item.selectionRange
        ? formatLspRange(item.selectionRange)
        : undefined,
      detail: item.detail ?? undefined,
      item,
    }));
    return textResult(JSON.stringify({ items: payload }, null, 2));
  }
}

export class STCallHierarchyIncomingTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<{ item: unknown }>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { item } = options.input;
    if (!item) {
      return errorResult("item is required.");
    }
    const result = await this.request("callHierarchy/incomingCalls", { item }, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const calls = Array.isArray(result.response) ? result.response : [];
    const payload = calls.map((call) => ({
      from: call.from?.name,
      fromUri: call.from?.uri ? formatUriString(call.from.uri) : undefined,
      fromRange: call.from?.range ? formatLspRange(call.from.range) : undefined,
      fromRanges: Array.isArray(call.fromRanges)
        ? call.fromRanges.map(
            (range: { start: { line: number; character: number }; end: { line: number; character: number } }) =>
              formatLspRange(range),
          )
        : [],
      call,
    }));
    return textResult(JSON.stringify({ calls: payload }, null, 2));
  }
}

export class STCallHierarchyOutgoingTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<{ item: unknown }>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { item } = options.input;
    if (!item) {
      return errorResult("item is required.");
    }
    const result = await this.request("callHierarchy/outgoingCalls", { item }, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const calls = Array.isArray(result.response) ? result.response : [];
    const payload = calls.map((call) => ({
      to: call.to?.name,
      toUri: call.to?.uri ? formatUriString(call.to.uri) : undefined,
      toRange: call.to?.range ? formatLspRange(call.to.range) : undefined,
      fromRanges: Array.isArray(call.fromRanges)
        ? call.fromRanges.map(
            (range: { start: { line: number; character: number }; end: { line: number; character: number } }) =>
              formatLspRange(range),
          )
        : [],
      call,
    }));
    return textResult(JSON.stringify({ calls: payload }, null, 2));
  }
}

export class STTypeHierarchyPrepareTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<PositionParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, line, character } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const doc = await ensureDocument(uri);
    const position = resolvePosition(doc, line, character);
    const params = {
      textDocument: { uri: uri.toString() },
      position: resolveLspPosition(position),
    };
    const result = await this.request("textDocument/prepareTypeHierarchy", params, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const items = Array.isArray(result.response) ? result.response : [];
    const payload = items.map((item) => ({
      name: item.name,
      kind: item.kind ? vscode.SymbolKind[item.kind] : undefined,
      uri: formatUriString(item.uri),
      range: item.range ? formatLspRange(item.range) : undefined,
      selectionRange: item.selectionRange
        ? formatLspRange(item.selectionRange)
        : undefined,
      detail: item.detail ?? undefined,
      item,
    }));
    return textResult(JSON.stringify({ items: payload }, null, 2));
  }
}

export class STTypeHierarchySupertypesTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<{ item: unknown }>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { item } = options.input;
    if (!item) {
      return errorResult("item is required.");
    }
    const result = await this.request("typeHierarchy/supertypes", { item }, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const items = Array.isArray(result.response) ? result.response : [];
    const payload = items.map((entry) => ({
      name: entry.name,
      kind: entry.kind ? vscode.SymbolKind[entry.kind] : undefined,
      uri: formatUriString(entry.uri),
      range: entry.range ? formatLspRange(entry.range) : undefined,
      selectionRange: entry.selectionRange
        ? formatLspRange(entry.selectionRange)
        : undefined,
      detail: entry.detail ?? undefined,
      item: entry,
    }));
    return textResult(JSON.stringify({ items: payload }, null, 2));
  }
}

export class STTypeHierarchySubtypesTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<{ item: unknown }>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { item } = options.input;
    if (!item) {
      return errorResult("item is required.");
    }
    const result = await this.request("typeHierarchy/subtypes", { item }, token);
    if ("error" in result) {
      return errorResult(result.error);
    }
    const items = Array.isArray(result.response) ? result.response : [];
    const payload = items.map((entry) => ({
      name: entry.name,
      kind: entry.kind ? vscode.SymbolKind[entry.kind] : undefined,
      uri: formatUriString(entry.uri),
      range: entry.range ? formatLspRange(entry.range) : undefined,
      selectionRange: entry.selectionRange
        ? formatLspRange(entry.selectionRange)
        : undefined,
      detail: entry.detail ?? undefined,
      item: entry,
    }));
    return textResult(JSON.stringify({ items: payload }, null, 2));
  }
}

export class STWorkspaceRenameFileTool {
  async invoke(
    options: InvocationOptions<WorkspaceFileRenameParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { oldPath, newPath, overwrite, useWorkspaceEdit } = options.input;
    const oldUri = uriFromFilePath(oldPath);
    const newUri = uriFromFilePath(newPath);
    if (!oldUri || !newUri) {
      return errorResult("oldPath/newPath must be absolute paths.");
    }
    const oldWorkspaceError = ensureWorkspaceUri(oldUri);
    if (oldWorkspaceError) {
      return errorResult(oldWorkspaceError);
    }
    const newWorkspaceError = ensureWorkspaceUri(newUri);
    if (newWorkspaceError) {
      return errorResult(newWorkspaceError);
    }
    try {
      if (useWorkspaceEdit ?? true) {
        const edit = new vscode.WorkspaceEdit();
        edit.renameFile(oldUri, newUri, { overwrite: overwrite ?? false });
        await vscode.workspace.applyEdit(edit);
      } else {
        await vscode.workspace.fs.rename(oldUri, newUri, {
          overwrite: overwrite ?? false,
        });
      }
      return textResult("File rename/move applied.");
    } catch (error) {
      return errorResult(`Failed to rename/move file: ${String(error)}`);
    }
  }
}

export class STSettingsUpdateTool extends LspToolBase {
  constructor(getClient?: LspClientProvider) {
    super(getClient);
  }

  async invoke(
    options: InvocationOptions<SettingsToggleParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { key, value, scope, filePath, timeoutMs, forceRefresh } = options.input;
    if (!key.trim()) {
      return errorResult("key must be a non-empty string.");
    }
    const split = key.split(".");
    const section = split.length > 1 ? split[0] : "trust-lsp";
    const setting = split.length > 1 ? split.slice(1).join(".") : key;
    const target =
      scope === "global"
        ? vscode.ConfigurationTarget.Global
        : scope === "workspaceFolder"
          ? vscode.ConfigurationTarget.WorkspaceFolder
          : vscode.ConfigurationTarget.Workspace;
    let configurationTarget: vscode.ConfigurationTarget | undefined = target;
    let scopeUri: vscode.Uri | undefined;
    if (target === vscode.ConfigurationTarget.WorkspaceFolder) {
      if (!filePath) {
        return errorResult("filePath is required for workspaceFolder scope.");
      }
      scopeUri = uriFromFilePath(filePath);
      if (!scopeUri) {
        return errorResult("filePath must be an absolute path or URI.");
      }
    }
    try {
      const config = vscode.workspace.getConfiguration(section, scopeUri);
      await config.update(setting, value, configurationTarget);
      const effectiveTimeoutMs =
        typeof timeoutMs === "number" ? timeoutMs : forceRefresh ? 3000 : 1000;
      let targetUri: vscode.Uri | undefined;
      if (filePath) {
        targetUri = uriFromFilePath(filePath);
        if (!targetUri) {
          return errorResult("filePath must be an absolute path or URI.");
        }
      } else {
        targetUri = vscode.window.activeTextEditor?.document.uri;
      }
      if (!targetUri) {
        return textResult(
          JSON.stringify(
            {
              setting: `${section}.${setting}`,
              diagnosticsRefreshed: false,
              reason: "No active document available for diagnostics refresh.",
            },
            null,
            2,
          ),
        );
      }
      await ensureDocument(targetUri);
      let diagnosticsRefreshed = false;
      let pullDiagnostics: unknown | undefined;
      let refreshError: string | undefined;
      if (forceRefresh) {
        const result = await this.request(
          "textDocument/diagnostic",
          { textDocument: { uri: targetUri.toString() } },
          token,
          { requestTimeoutMs: effectiveTimeoutMs },
        );
        if ("error" in result) {
          refreshError = result.error;
        } else {
          pullDiagnostics = result.response;
          diagnosticsRefreshed = true;
        }
      }
      const waited = await waitForDiagnostics(
        targetUri,
        token,
        effectiveTimeoutMs,
      );
      diagnosticsRefreshed = diagnosticsRefreshed || waited;
      const diagnostics = vscode.languages.getDiagnostics(targetUri);
      return textResult(
        JSON.stringify(
          {
            setting: `${section}.${setting}`,
            diagnosticsRefreshed,
            diagnostics: diagnosticsPayload(diagnostics).diagnostics,
            pullDiagnostics: pullDiagnostics ?? undefined,
            refreshError: refreshError ?? undefined,
          },
          null,
          2,
        ),
      );
    } catch (error) {
      return errorResult(`Failed to update setting: ${String(error)}`);
    }
  }
}

export class STTelemetryReadTool {
  async invoke(
    options: InvocationOptions<TelemetryReadParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, limit, tail } = options.input;
    const workspaceFolders = vscode.workspace.workspaceFolders ?? [];
    let uri: vscode.Uri | undefined = undefined;
    if (filePath) {
      uri = uriFromFilePath(filePath);
      if (!uri) {
        return errorResult("filePath must be an absolute path or URI.");
      }
    } else {
      for (const folder of workspaceFolders) {
        const candidate = vscode.Uri.joinPath(
          folder.uri,
          ".trust-lsp",
          "telemetry.jsonl",
        );
        try {
          await vscode.workspace.fs.stat(candidate);
          uri = candidate;
          break;
        } catch {
          continue;
        }
      }
    }
    if (!uri) {
      return errorResult("Telemetry file not found.");
    }
    const workspaceError = ensureWorkspaceUri(uri);
    if (workspaceError) {
      return errorResult(workspaceError);
    }
    try {
      const bytes = await vscode.workspace.fs.readFile(uri);
      const text = new TextDecoder().decode(bytes);
      const lines = text.split(/\r?\n/).filter((line) => line.trim().length > 0);
      const maxItems = limit ?? 100;
      const slice = tail ? lines.slice(-maxItems) : lines.slice(0, maxItems);
      const items = slice.map((line) => {
        try {
          return JSON.parse(line);
        } catch {
          return { parseError: true, line };
        }
      });
      return textResult(
        JSON.stringify(
          {
            filePath: uri.fsPath,
            totalLines: lines.length,
            items,
            truncated: lines.length > slice.length,
          },
          null,
          2,
        ),
      );
    } catch (error) {
      return errorResult(`Failed to read telemetry: ${String(error)}`);
    }
  }
}

export class STInlineValuesTool {
  async invoke(
    options: InvocationOptions<InlineValuesParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const {
      frameId,
      startLine,
      startCharacter,
      endLine,
      endCharacter,
      context,
    } = options.input;
    if (!Number.isInteger(frameId)) {
      return errorResult("frameId must be an integer.");
    }
    const session = vscode.debug.activeDebugSession;
    if (!session) {
      return errorResult("No active debug session.");
    }
    const range = {
      start: { line: startLine + 1, column: startCharacter + 1 },
      end: { line: endLine + 1, column: endCharacter + 1 },
    };
    try {
      const inlineValues = await session.customRequest("inlineValues", {
        frameId,
        range,
        context,
      });
      return textResult(
        JSON.stringify({ inlineValues, session: session.name }, null, 2),
      );
    } catch (error) {
      return errorResult(`Failed to fetch inline values: ${String(error)}`);
    }
  }
}

export class STProjectInfoTool extends LspToolBase {
  async invoke(
    options: InvocationOptions<ProjectInfoParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const args = Array.isArray(options.input.arguments)
      ? options.input.arguments
      : [];
    const result = await this.request(
      "workspace/executeCommand",
      { command: "trust-lsp.projectInfo", arguments: args },
      token,
    );
    if ("error" in result) {
      return errorResult(result.error);
    }
    return textResult(
      JSON.stringify(
        {
          command: "trust-lsp.projectInfo",
          result: result.response,
        },
        null,
        2,
      ),
    );
  }
}

export class STWorkspaceSymbolsTimedTool {
  async invoke(
    options: InvocationOptions<WorkspaceSymbolsTimedParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { query, limit, pathIncludes } = options.input;
    if (!query.trim()) {
      return errorResult("query must be a non-empty string.");
    }
    const start = Date.now();
    try {
      const symbols = await vscode.commands.executeCommand<
        vscode.SymbolInformation[]
      >("vscode.executeWorkspaceSymbolProvider", query);
      const durationMs = Date.now() - start;
      if (!symbols || symbols.length === 0) {
        return textResult(
          JSON.stringify({ durationMs, symbols: [] }, null, 2),
        );
      }
      let filtered = symbols;
      if (Array.isArray(pathIncludes) && pathIncludes.length > 0) {
        filtered = symbols.filter((symbol) =>
          pathIncludes.some((part) =>
            formatLocationLike(symbol.location).includes(part),
          ),
        );
      }
      const { items, truncated } = truncateItems(filtered, limit ?? MAX_ITEMS);
      const payload = items.map((symbol) => ({
        name: symbol.name,
        kind: symbolKindName(symbol.kind),
        containerName: symbol.containerName || undefined,
        location: formatLocationLike(symbol.location),
      }));
      return textResult(
        JSON.stringify({ durationMs, symbols: payload, truncated }, null, 2),
      );
    } catch (error) {
      return errorResult(`Failed to get workspace symbols: ${String(error)}`);
    }
  }
}

export class STFileReadTool {
  async invoke(
    options: InvocationOptions<FileReadParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, startLine, startCharacter, endLine, endCharacter } =
      options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const workspaceError = ensureWorkspaceUri(uri);
    if (workspaceError) {
      return errorResult(workspaceError);
    }
    const hasRangePart =
      startLine !== undefined ||
      startCharacter !== undefined ||
      endLine !== undefined ||
      endCharacter !== undefined;
    const hasFullRange =
      startLine !== undefined &&
      startCharacter !== undefined &&
      endLine !== undefined &&
      endCharacter !== undefined;
    if (hasRangePart && !hasFullRange) {
      return errorResult(
        "Provide all of startLine/startCharacter/endLine/endCharacter for range reads.",
      );
    }
    try {
      const doc = await ensureDocument(uri);
      if (
        startLine !== undefined &&
        startCharacter !== undefined &&
        endLine !== undefined &&
        endCharacter !== undefined
      ) {
        const range = resolveRange(
          doc,
          startLine,
          startCharacter,
          endLine,
          endCharacter,
        );
        const text = doc.getText(range);
        return textResult(
          JSON.stringify(
            {
              filePath: uri.fsPath,
              range: formatRange(range),
              text,
            },
            null,
            2,
          ),
        );
      }
      const text = doc.getText();
      return textResult(
        JSON.stringify({ filePath: uri.fsPath, text }, null, 2),
      );
    } catch (error) {
      return errorResult(`Failed to read file: ${String(error)}`);
    }
  }
}

export class STReadRangeTool {
  async invoke(
    options: InvocationOptions<RangeParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, startLine, startCharacter, endLine, endCharacter } =
      options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const workspaceError = ensureWorkspaceUri(uri);
    if (workspaceError) {
      return errorResult(workspaceError);
    }
    try {
      const doc = await ensureDocument(uri);
      const range = resolveRange(
        doc,
        startLine,
        startCharacter,
        endLine,
        endCharacter,
      );
      const text = doc.getText(range);
      return textResult(
        JSON.stringify(
          {
            filePath: uri.fsPath,
            range: formatRange(range),
            text,
          },
          null,
          2,
        ),
      );
    } catch (error) {
      return errorResult(`Failed to read range: ${String(error)}`);
    }
  }
}

export class STFileWriteTool {
  async invoke(
    options: InvocationOptions<FileWriteParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, text, save } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const workspaceError = ensureWorkspaceUri(uri);
    if (workspaceError) {
      return errorResult(workspaceError);
    }
    try {
      const openDoc = openDocumentIfLoaded(uri);
      if (openDoc) {
        const edit = new vscode.WorkspaceEdit();
        edit.replace(uri, fullDocumentRange(openDoc), text);
        await vscode.workspace.applyEdit(edit);
        if (save) {
          await openDoc.save();
        }
        return textResult("File updated.");
      }
      const encoder = new TextEncoder();
      await vscode.workspace.fs.writeFile(uri, encoder.encode(text));
      if (save) {
        const doc = await ensureDocument(uri);
        await doc.save();
      }
      return textResult("File written.");
    } catch (error) {
      return errorResult(`Failed to write file: ${String(error)}`);
    }
  }
}

export class STApplyEditsTool {
  async invoke(
    options: InvocationOptions<ApplyEditsParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const { filePath, edits, save } = options.input;
    const uri = uriFromFilePath(filePath);
    if (!uri) {
      return errorResult("filePath must be an absolute path or URI.");
    }
    const workspaceError = ensureWorkspaceUri(uri);
    if (workspaceError) {
      return errorResult(workspaceError);
    }
    if (!Array.isArray(edits) || edits.length === 0) {
      return errorResult("edits must be a non-empty array.");
    }
    try {
      const doc = await ensureDocument(uri);
      const workspaceEdit = new vscode.WorkspaceEdit();
      for (const edit of edits) {
        const range = resolveRange(
          doc,
          edit.startLine,
          edit.startCharacter,
          edit.endLine,
          edit.endCharacter,
        );
        workspaceEdit.replace(uri, range, edit.newText);
      }
      await vscode.workspace.applyEdit(workspaceEdit);
      if (save) {
        await doc.save();
      }
      return textResult("Edits applied.");
    } catch (error) {
      return errorResult(`Failed to apply edits: ${String(error)}`);
    }
  }
}

export class STDebugStartTool {
  async invoke(
    options: InvocationOptions<DebugStartParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    const uri = optionalUriFromFilePath(options.input.filePath);
    if (options.input.filePath && !uri) {
      return errorResult("filePath must be an absolute path or URI when provided.");
    }
    try {
      await vscode.commands.executeCommand("trust-lsp.debug.start", uri);
      return textResult("Debug start requested.");
    } catch (error) {
      return errorResult(`Failed to start debugging: ${String(error)}`);
    }
  }
}

export class STDebugAttachTool {
  async invoke(
    _options: InvocationOptions<EmptyParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    try {
      await vscode.commands.executeCommand("trust-lsp.debug.attach");
      return textResult("Debug attach requested.");
    } catch (error) {
      return errorResult(`Failed to attach debugger: ${String(error)}`);
    }
  }
}

export class STDebugReloadTool {
  async invoke(
    _options: InvocationOptions<EmptyParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    try {
      await vscode.commands.executeCommand("trust-lsp.debug.reload");
      return textResult("Debug reload requested.");
    } catch (error) {
      return errorResult(`Failed to reload debugger: ${String(error)}`);
    }
  }
}

export class STDebugOpenIoPanelTool {
  async invoke(
    _options: InvocationOptions<EmptyParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    try {
      await vscode.commands.executeCommand("trust-lsp.debug.openIoPanel");
      return textResult("Opened I/O panel.");
    } catch (error) {
      return errorResult(`Failed to open I/O panel: ${String(error)}`);
    }
  }
}

export class STDebugEnsureConfigurationTool {
  async invoke(
    _options: InvocationOptions<EmptyParams>,
    token: vscode.CancellationToken,
  ): Promise<unknown> {
    if (token.isCancellationRequested) {
      return textResult("Cancelled.");
    }
    try {
      await vscode.commands.executeCommand("trust-lsp.debug.ensureConfiguration");
      return textResult("Ensure configuration requested.");
    } catch (error) {
      return errorResult(`Failed to ensure configuration: ${String(error)}`);
    }
  }
}

export function registerLanguageModelTools(
  context: vscode.ExtensionContext,
  options?: { getClient?: LspClientProvider },
): void {
  if (!lmAvailable()) {
    return;
  }
  const lm = (vscode as unknown as { lm?: LmApi }).lm;
  if (!lm) {
    return;
  }
  const getClient = options?.getClient;
  context.subscriptions.push(
    lm.registerTool("trust_lsp_request", new STLspRequestTool(getClient)),
    lm.registerTool("trust_lsp_notify", new STLspNotificationTool(getClient)),
    lm.registerTool("trust_get_hover", new STHoverTool()),
    lm.registerTool(
      "trust_get_semantic_tokens_full",
      new STSemanticTokensFullTool(getClient),
    ),
    lm.registerTool(
      "trust_get_semantic_tokens_delta",
      new STSemanticTokensDeltaTool(getClient),
    ),
    lm.registerTool(
      "trust_get_semantic_tokens_range",
      new STSemanticTokensRangeTool(getClient),
    ),
    lm.registerTool("trust_get_inlay_hints", new STInlayHintsTool(getClient)),
    lm.registerTool(
      "trust_get_linked_editing",
      new STLinkedEditingTool(getClient),
    ),
    lm.registerTool(
      "trust_get_document_links",
      new STDocumentLinksTool(getClient),
    ),
    lm.registerTool("trust_get_code_lens", new STCodeLensTool(getClient)),
    lm.registerTool(
      "trust_get_selection_ranges",
      new STSelectionRangeTool(getClient),
    ),
    lm.registerTool(
      "trust_get_on_type_formatting_edits",
      new STOnTypeFormattingTool(getClient),
    ),
    lm.registerTool(
      "trust_call_hierarchy_prepare",
      new STCallHierarchyPrepareTool(getClient),
    ),
    lm.registerTool(
      "trust_call_hierarchy_incoming",
      new STCallHierarchyIncomingTool(getClient),
    ),
    lm.registerTool(
      "trust_call_hierarchy_outgoing",
      new STCallHierarchyOutgoingTool(getClient),
    ),
    lm.registerTool(
      "trust_type_hierarchy_prepare",
      new STTypeHierarchyPrepareTool(getClient),
    ),
    lm.registerTool(
      "trust_type_hierarchy_supertypes",
      new STTypeHierarchySupertypesTool(getClient),
    ),
    lm.registerTool(
      "trust_type_hierarchy_subtypes",
      new STTypeHierarchySubtypesTool(getClient),
    ),
    lm.registerTool("trust_file_read", new STFileReadTool()),
    lm.registerTool("trust_read_range", new STReadRangeTool()),
    lm.registerTool("trust_file_write", new STFileWriteTool()),
    lm.registerTool("trust_apply_edits", new STApplyEditsTool()),
    lm.registerTool("trust_get_diagnostics", new STDiagnosticsTool()),
    lm.registerTool("trust_get_definition", new STDefinitionTool()),
    lm.registerTool("trust_get_declaration", new STDeclarationTool()),
    lm.registerTool("trust_get_type_definition", new STTypeDefinitionTool()),
    lm.registerTool("trust_get_implementation", new STImplementationTool()),
    lm.registerTool("trust_get_references", new STReferencesTool()),
    lm.registerTool("trust_get_completions", new STCompletionTool()),
    lm.registerTool("trust_get_signature_help", new STSignatureHelpTool()),
    lm.registerTool("trust_get_document_symbols", new STDocumentSymbolsTool()),
    lm.registerTool("trust_get_workspace_symbols", new STWorkspaceSymbolsTool()),
    lm.registerTool(
      "trust_get_workspace_symbols_timed",
      new STWorkspaceSymbolsTimedTool(),
    ),
    lm.registerTool("trust_get_rename_edits", new STRenameTool()),
    lm.registerTool("trust_get_formatting_edits", new STFormatTool()),
    lm.registerTool("trust_get_code_actions", new STCodeActionsTool(getClient)),
    lm.registerTool("trust_get_project_info", new STProjectInfoTool(getClient)),
    lm.registerTool("trust_workspace_rename_file", new STWorkspaceRenameFileTool()),
    lm.registerTool("trust_update_settings", new STSettingsUpdateTool(getClient)),
    lm.registerTool("trust_read_telemetry", new STTelemetryReadTool()),
    lm.registerTool("trust_get_inline_values", new STInlineValuesTool()),
    lm.registerTool("trust_debug_start", new STDebugStartTool()),
    lm.registerTool("trust_debug_attach", new STDebugAttachTool()),
    lm.registerTool("trust_debug_reload", new STDebugReloadTool()),
    lm.registerTool("trust_debug_open_io_panel", new STDebugOpenIoPanelTool()),
    lm.registerTool(
      "trust_debug_ensure_configuration",
      new STDebugEnsureConfigurationTool(),
    ),
  );
}
