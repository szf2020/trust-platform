//! Browser/WASM analysis adapter for truST.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use text_size::{TextRange, TextSize};
use trust_hir::db::FileId;
use trust_hir::project::{Project, SourceKey};
use trust_hir::DiagnosticSeverity;
use trust_ide::StdlibFilter;

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
use wasm_bindgen::prelude::wasm_bindgen;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    message: String,
}

impl EngineError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EngineError {}

type EngineResult<T> = Result<T, EngineError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentInput {
    pub uri: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadedDocument {
    pub uri: String,
    pub file_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplyDocumentsResult {
    pub documents: Vec<LoadedDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineStatus {
    pub document_count: usize,
    pub uris: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelatedInfoItem {
    pub range: Range,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticItem {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub range: Range,
    pub related: Vec<RelatedInfoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HoverRequest {
    pub uri: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HoverItem {
    pub contents: String,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionRequest {
    pub uri: String,
    pub position: Position,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionTextEditItem {
    pub range: Range,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: Option<String>,
    pub text_edit: Option<CompletionTextEditItem>,
    pub sort_priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferencesRequest {
    pub uri: String,
    pub position: Position,
    pub include_declaration: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferenceItem {
    pub uri: String,
    pub range: Range,
    pub is_write: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionRequest {
    pub uri: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefinitionItem {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentHighlightRequest {
    pub uri: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentHighlightItem {
    pub range: Range,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenameRequest {
    pub uri: String,
    pub position: Position,
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenameEdit {
    pub uri: String,
    pub range: Range,
    pub new_text: String,
}

#[derive(Debug, Default)]
pub struct BrowserAnalysisEngine {
    project: Project,
    documents: BTreeMap<String, String>,
}

impl BrowserAnalysisEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace_documents(
        &mut self,
        documents: Vec<DocumentInput>,
    ) -> EngineResult<ApplyDocumentsResult> {
        let mut seen = BTreeSet::new();
        for document in &documents {
            let uri = document.uri.trim();
            if uri.is_empty() {
                return Err(EngineError::new("document uri must not be empty"));
            }
            if !seen.insert(uri.to_string()) {
                return Err(EngineError::new(format!(
                    "duplicate document uri '{uri}' in request"
                )));
            }
        }

        let incoming_uris: BTreeSet<String> = documents.iter().map(|doc| doc.uri.clone()).collect();
        let stale: Vec<String> = self
            .documents
            .keys()
            .filter(|uri| !incoming_uris.contains(*uri))
            .cloned()
            .collect();
        for uri in stale {
            let key = source_key(&uri);
            let _ = self.project.remove_source(&key);
            self.documents.remove(&uri);
        }

        let mut loaded = Vec::with_capacity(documents.len());
        for document in documents {
            let file_id = self
                .project
                .set_source_text(source_key(&document.uri), document.text.clone());
            self.documents.insert(document.uri.clone(), document.text);
            loaded.push(LoadedDocument {
                uri: document.uri,
                file_id: file_id.0,
            });
        }

        Ok(ApplyDocumentsResult { documents: loaded })
    }

    pub fn diagnostics(&self, uri: &str) -> EngineResult<Vec<DiagnosticItem>> {
        let file_id = self.file_id_for_uri(uri)?;
        let source = self.source_for_uri(uri)?;
        let diagnostics = self
            .project
            .with_database(|db| trust_ide::diagnostics::collect_diagnostics(db, file_id));
        let mut items: Vec<DiagnosticItem> = diagnostics
            .into_iter()
            .map(|diagnostic| {
                let mut related = diagnostic
                    .related
                    .into_iter()
                    .map(|related| RelatedInfoItem {
                        range: lsp_range(source, related.range),
                        message: related.message,
                    })
                    .collect::<Vec<_>>();
                related.sort_by(|left, right| {
                    left.range
                        .cmp(&right.range)
                        .then_with(|| left.message.cmp(&right.message))
                });
                DiagnosticItem {
                    code: diagnostic.code.code().to_string(),
                    severity: severity_label(diagnostic.severity).to_string(),
                    message: diagnostic.message,
                    range: lsp_range(source, diagnostic.range),
                    related,
                }
            })
            .collect();
        items.sort_by(|left, right| {
            left.range
                .cmp(&right.range)
                .then_with(|| left.code.cmp(&right.code))
                .then_with(|| left.message.cmp(&right.message))
                .then_with(|| left.severity.cmp(&right.severity))
        });
        Ok(items)
    }

    pub fn hover(&self, request: HoverRequest) -> EngineResult<Option<HoverItem>> {
        let file_id = self.file_id_for_uri(&request.uri)?;
        let source = self.source_for_uri(&request.uri)?;
        let Some(offset) = position_to_offset(source, request.position.clone()) else {
            return Err(EngineError::new(format!(
                "position {}:{} is outside document '{}'",
                request.position.line, request.position.character, request.uri
            )));
        };
        let result = self.project.with_database(|db| {
            trust_ide::hover_with_filter(
                db,
                file_id,
                TextSize::from(offset),
                &StdlibFilter::allow_all(),
            )
        });
        Ok(result.map(|hover| HoverItem {
            contents: hover.contents,
            range: hover.range.map(|range| lsp_range(source, range)),
        }))
    }

    pub fn completion(&self, request: CompletionRequest) -> EngineResult<Vec<CompletionItem>> {
        let file_id = self.file_id_for_uri(&request.uri)?;
        let source = self.source_for_uri(&request.uri)?;
        let Some(offset) = position_to_offset(source, request.position.clone()) else {
            return Err(EngineError::new(format!(
                "position {}:{} is outside document '{}'",
                request.position.line, request.position.character, request.uri
            )));
        };
        let mut items = self.project.with_database(|db| {
            trust_ide::complete_with_filter(
                db,
                file_id,
                TextSize::from(offset),
                &StdlibFilter::allow_all(),
            )
        });
        let typed_prefix = completion_prefix_at_offset(source, offset);
        items.sort_by(|left, right| {
            completion_match_rank(left.label.as_str(), typed_prefix.as_deref())
                .cmp(&completion_match_rank(
                    right.label.as_str(),
                    typed_prefix.as_deref(),
                ))
                .then_with(|| left.sort_priority.cmp(&right.sort_priority))
                .then_with(|| left.label.cmp(&right.label))
        });
        let limit = request.limit.unwrap_or(50).clamp(1, 500) as usize;
        let completion = items
            .into_iter()
            .take(limit)
            .map(|item| CompletionItem {
                label: item.label.to_string(),
                kind: completion_kind_label(item.kind).to_string(),
                detail: item.detail.map(|value| value.to_string()),
                documentation: item.documentation.map(|value| value.to_string()),
                insert_text: item.insert_text.map(|value| value.to_string()),
                text_edit: item.text_edit.map(|edit| CompletionTextEditItem {
                    range: lsp_range(source, edit.range),
                    new_text: edit.new_text.to_string(),
                }),
                sort_priority: item.sort_priority,
            })
            .collect();
        Ok(completion)
    }

    pub fn references(&self, request: ReferencesRequest) -> EngineResult<Vec<ReferenceItem>> {
        let file_id = self.file_id_for_uri(&request.uri)?;
        let source = self.source_for_uri(&request.uri)?;
        let Some(offset) = position_to_offset(source, request.position.clone()) else {
            return Err(EngineError::new(format!(
                "position {}:{} is outside document '{}'",
                request.position.line, request.position.character, request.uri
            )));
        };
        let include_declaration = request.include_declaration.unwrap_or(true);
        let refs = self.project.with_database(|db| {
            trust_ide::find_references(
                db,
                file_id,
                TextSize::from(offset),
                trust_ide::FindReferencesOptions {
                    include_declaration,
                },
            )
        });
        let mut items: Vec<ReferenceItem> = refs
            .into_iter()
            .filter_map(|reference| {
                let ref_uri = self.uri_for_file_id(reference.file_id)?;
                let ref_source = self.source_for_uri(&ref_uri).ok()?;
                Some(ReferenceItem {
                    uri: ref_uri,
                    range: lsp_range(ref_source, reference.range),
                    is_write: reference.is_write,
                })
            })
            .collect();
        items.sort_by(|a, b| a.uri.cmp(&b.uri).then_with(|| a.range.cmp(&b.range)));
        Ok(items)
    }

    pub fn definition(&self, request: DefinitionRequest) -> EngineResult<Option<DefinitionItem>> {
        let file_id = self.file_id_for_uri(&request.uri)?;
        let source = self.source_for_uri(&request.uri)?;
        let Some(offset) = position_to_offset(source, request.position.clone()) else {
            return Err(EngineError::new(format!(
                "position {}:{} is outside document '{}'",
                request.position.line, request.position.character, request.uri
            )));
        };
        let result = self
            .project
            .with_database(|db| trust_ide::goto_definition(db, file_id, TextSize::from(offset)));
        let item = result.and_then(|def| {
            let def_uri = self.uri_for_file_id(def.file_id)?;
            let def_source = self.source_for_uri(&def_uri).ok()?;
            Some(DefinitionItem {
                uri: def_uri,
                range: lsp_range(def_source, def.range),
            })
        });
        Ok(item)
    }

    pub fn document_highlight(
        &self,
        request: DocumentHighlightRequest,
    ) -> EngineResult<Vec<DocumentHighlightItem>> {
        let file_id = self.file_id_for_uri(&request.uri)?;
        let source = self.source_for_uri(&request.uri)?;
        let Some(offset) = position_to_offset(source, request.position.clone()) else {
            return Err(EngineError::new(format!(
                "position {}:{} is outside document '{}'",
                request.position.line, request.position.character, request.uri
            )));
        };
        let refs = self.project.with_database(|db| {
            trust_ide::find_references(
                db,
                file_id,
                TextSize::from(offset),
                trust_ide::FindReferencesOptions {
                    include_declaration: true,
                },
            )
        });
        let mut items: Vec<DocumentHighlightItem> = refs
            .into_iter()
            .filter(|reference| reference.file_id == file_id)
            .map(|reference| DocumentHighlightItem {
                range: lsp_range(source, reference.range),
                kind: if reference.is_write {
                    "write".to_string()
                } else {
                    "read".to_string()
                },
            })
            .collect();
        items.sort_by(|a, b| a.range.cmp(&b.range));
        Ok(items)
    }

    pub fn rename(&self, request: RenameRequest) -> EngineResult<Vec<RenameEdit>> {
        let file_id = self.file_id_for_uri(&request.uri)?;
        let source = self.source_for_uri(&request.uri)?;
        let Some(offset) = position_to_offset(source, request.position.clone()) else {
            return Err(EngineError::new(format!(
                "position {}:{} is outside document '{}'",
                request.position.line, request.position.character, request.uri
            )));
        };
        let result = self.project.with_database(|db| {
            trust_ide::rename(db, file_id, TextSize::from(offset), &request.new_name)
        });
        let Some(rename_result) = result else {
            return Ok(vec![]);
        };
        let mut edits: Vec<RenameEdit> = rename_result
            .edits
            .into_iter()
            .filter_map(|(edit_file_id, file_edits)| {
                let edit_uri = self.uri_for_file_id(edit_file_id)?;
                let edit_source = self.source_for_uri(&edit_uri).ok()?;
                Some(file_edits.into_iter().map(move |edit| RenameEdit {
                    uri: edit_uri.clone(),
                    range: lsp_range(edit_source, edit.range),
                    new_text: edit.new_text,
                }))
            })
            .flatten()
            .collect();
        edits.sort_by(|a, b| a.uri.cmp(&b.uri).then_with(|| a.range.cmp(&b.range)));
        Ok(edits)
    }

    pub fn status(&self) -> EngineStatus {
        EngineStatus {
            document_count: self.documents.len(),
            uris: self.documents.keys().cloned().collect(),
        }
    }

    fn source_for_uri(&self, uri: &str) -> EngineResult<&str> {
        self.documents
            .get(uri)
            .map(String::as_str)
            .ok_or_else(|| EngineError::new(format!("document '{uri}' is not loaded")))
    }

    fn file_id_for_uri(&self, uri: &str) -> EngineResult<FileId> {
        let key = source_key(uri);
        self.project
            .file_id_for_key(&key)
            .ok_or_else(|| EngineError::new(format!("document '{uri}' is not loaded")))
    }

    fn uri_for_file_id(&self, file_id: FileId) -> Option<String> {
        let key = self.project.key_for_file_id(file_id)?;
        Some(key.display())
    }
}

#[cfg_attr(all(target_arch = "wasm32", feature = "wasm"), wasm_bindgen)]
pub struct WasmAnalysisEngine {
    inner: BrowserAnalysisEngine,
}

#[cfg_attr(all(target_arch = "wasm32", feature = "wasm"), wasm_bindgen)]
impl WasmAnalysisEngine {
    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(constructor)
    )]
    pub fn new() -> Self {
        Self {
            inner: BrowserAnalysisEngine::new(),
        }
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = applyDocumentsJson)
    )]
    pub fn apply_documents_json(&mut self, documents_json: &str) -> Result<String, String> {
        let documents: Vec<DocumentInput> = serde_json::from_str(documents_json)
            .map_err(|err| format!("invalid documents json: {err}"))?;
        let result = self.inner.replace_documents(documents)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = diagnosticsJson)
    )]
    pub fn diagnostics_json(&self, uri: &str) -> Result<String, String> {
        let result = self.inner.diagnostics(uri)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = hoverJson)
    )]
    pub fn hover_json(&self, request_json: &str) -> Result<String, String> {
        let request: HoverRequest = serde_json::from_str(request_json)
            .map_err(|err| format!("invalid hover request json: {err}"))?;
        let result = self.inner.hover(request)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = completionJson)
    )]
    pub fn completion_json(&self, request_json: &str) -> Result<String, String> {
        let request: CompletionRequest = serde_json::from_str(request_json)
            .map_err(|err| format!("invalid completion request json: {err}"))?;
        let result = self.inner.completion(request)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = referencesJson)
    )]
    pub fn references_json(&self, request_json: &str) -> Result<String, String> {
        let request: ReferencesRequest = serde_json::from_str(request_json)
            .map_err(|err| format!("invalid references request json: {err}"))?;
        let result = self.inner.references(request)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = definitionJson)
    )]
    pub fn definition_json(&self, request_json: &str) -> Result<String, String> {
        let request: DefinitionRequest = serde_json::from_str(request_json)
            .map_err(|err| format!("invalid definition request json: {err}"))?;
        let result = self.inner.definition(request)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = documentHighlightJson)
    )]
    pub fn document_highlight_json(&self, request_json: &str) -> Result<String, String> {
        let request: DocumentHighlightRequest = serde_json::from_str(request_json)
            .map_err(|err| format!("invalid documentHighlight request json: {err}"))?;
        let result = self.inner.document_highlight(request)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = renameJson)
    )]
    pub fn rename_json(&self, request_json: &str) -> Result<String, String> {
        let request: RenameRequest = serde_json::from_str(request_json)
            .map_err(|err| format!("invalid rename request json: {err}"))?;
        let result = self.inner.rename(request)?;
        json_string(&result)
    }

    #[cfg_attr(
        all(target_arch = "wasm32", feature = "wasm"),
        wasm_bindgen(js_name = statusJson)
    )]
    pub fn status_json(&self) -> Result<String, String> {
        json_string(&self.inner.status())
    }
}

impl Default for WasmAnalysisEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl From<EngineError> for String {
    fn from(value: EngineError) -> Self {
        value.to_string()
    }
}

fn json_string<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|err| format!("json serialization failed: {err}"))
}

fn source_key(uri: &str) -> SourceKey {
    SourceKey::from_virtual(uri.to_string())
}

fn completion_prefix_at_offset(source: &str, offset: u32) -> Option<String> {
    let bytes = source.as_bytes();
    let mut cursor = (offset as usize).min(bytes.len());
    let end = cursor;
    while cursor > 0 && is_ident_byte(bytes[cursor - 1]) {
        cursor -= 1;
    }
    if cursor == end {
        return None;
    }
    let prefix = &source[cursor..end];
    if prefix.is_empty() {
        return None;
    }
    Some(prefix.to_ascii_uppercase())
}

fn completion_match_rank(label: &str, typed_prefix: Option<&str>) -> u8 {
    let Some(prefix) = typed_prefix else {
        return 2;
    };
    if prefix.is_empty() {
        return 2;
    }
    let label_upper = label.to_ascii_uppercase();
    if label_upper == prefix {
        return 0;
    }
    if label_upper.starts_with(prefix) {
        return 1;
    }
    if label_upper.contains(prefix) {
        return 2;
    }
    3
}

fn is_ident_byte(byte: u8) -> bool {
    matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
}

fn completion_kind_label(kind: trust_ide::CompletionKind) -> &'static str {
    match kind {
        trust_ide::CompletionKind::Keyword => "keyword",
        trust_ide::CompletionKind::Function => "function",
        trust_ide::CompletionKind::FunctionBlock => "function_block",
        trust_ide::CompletionKind::Method => "method",
        trust_ide::CompletionKind::Property => "property",
        trust_ide::CompletionKind::Variable => "variable",
        trust_ide::CompletionKind::Constant => "constant",
        trust_ide::CompletionKind::Type => "type",
        trust_ide::CompletionKind::EnumValue => "enum_value",
        trust_ide::CompletionKind::Snippet => "snippet",
    }
}

fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Hint => "hint",
    }
}

fn lsp_range(content: &str, range: TextRange) -> Range {
    Range {
        start: offset_to_position(content, u32::from(range.start())),
        end: offset_to_position(content, u32::from(range.end())),
    }
}

fn offset_to_position(content: &str, offset: u32) -> Position {
    let clamped_offset = (offset as usize).min(content.len());
    let mut line = 0u32;
    let mut character = 0u32;
    for (index, ch) in content.char_indices() {
        if index >= clamped_offset {
            break;
        }
        if ch == '\n' {
            line = line.saturating_add(1);
            character = 0;
        } else {
            character = character.saturating_add(ch.len_utf16() as u32);
        }
    }
    Position { line, character }
}

fn position_to_offset(content: &str, position: Position) -> Option<u32> {
    let mut line = 0u32;
    let mut character = 0u32;
    for (index, ch) in content.char_indices() {
        if line == position.line {
            if character == position.character {
                return Some(index as u32);
            }
            if ch == '\n' {
                return Some(index as u32);
            }
            let width = ch.len_utf16() as u32;
            if character.saturating_add(width) > position.character {
                return Some(index as u32);
            }
            character = character.saturating_add(width);
            continue;
        }

        if ch == '\n' {
            line = line.saturating_add(1);
            character = 0;
        }
    }
    if line == position.line {
        Some(content.len() as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{offset_to_position, position_to_offset, Position};

    #[test]
    fn line_character_offset_roundtrip_ascii() {
        let source = "PROGRAM Main\nVAR\n  x : INT;\nEND_VAR\n";
        let position = Position {
            line: 2,
            character: 2,
        };
        let offset = position_to_offset(source, position.clone()).expect("offset");
        let roundtrip = offset_to_position(source, offset);
        assert_eq!(roundtrip, position);
    }

    #[test]
    fn line_character_offset_roundtrip_utf16() {
        let source = "PROGRAM Main\nVAR\n  emoji : STRING := '😀';\nEND_VAR\n";
        let position = Position {
            line: 2,
            character: 25,
        };
        let offset = position_to_offset(source, position.clone()).expect("offset");
        let roundtrip = offset_to_position(source, offset);
        assert_eq!(roundtrip, position);
    }

    #[test]
    fn position_to_offset_clamps_inside_utf16_surrogate_pair() {
        let source = "😀a";
        let offset = position_to_offset(
            source,
            Position {
                line: 0,
                character: 1,
            },
        )
        .expect("offset");
        assert_eq!(offset, 0);
    }
}
