use super::collector::SymbolCollector;
use super::diagnostics::{
    add_unused_symbol_warnings, check_abstract_instantiations, check_class_semantics,
    check_configuration_semantics, check_cyclomatic_complexity, check_extends_implements_semantics,
    check_global_external_links_with_project, check_interface_conformance, check_nondeterminism,
    check_property_accessors, check_shared_global_task_hazards, check_unreachable_statements,
    check_using_directives, collect_used_symbols, expression_by_id, expression_context,
    expression_id_at_offset, resolve_declared_var_types_with_project,
    resolve_pending_types_with_table, type_check_file,
};
use super::symbol_import::SymbolImporter;
use super::*;
use rustc_hash::FxHashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

impl Database {
    /// Creates a new empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all caches (call after any source change).
    ///
    /// Diagnostics and symbol queries can depend on cross-file state, so we
    /// treat any edit as a full invalidation for now.
    pub fn invalidate(&mut self, _file_id: FileId) {
        let mut revision = self.revision.write().expect("revision lock poisoned");
        *revision = revision.wrapping_add(1);
        self.symbol_cache
            .write()
            .expect("symbol cache poisoned")
            .clear();
        self.analysis_cache
            .write()
            .expect("analysis cache poisoned")
            .clear();
        self.expr_cache
            .write()
            .expect("expression cache poisoned")
            .clear();
    }

    fn project_symbol_tables(&self) -> FxHashMap<FileId, Arc<SymbolTable>> {
        let mut tables = FxHashMap::default();
        for &file_id in self.sources.keys() {
            tables.insert(file_id, self.file_symbols(file_id));
        }
        tables
    }

    fn ordered_table_entries<'a>(
        &self,
        tables: &'a FxHashMap<FileId, Arc<SymbolTable>>,
    ) -> Vec<(FileId, &'a Arc<SymbolTable>)> {
        let mut entries: Vec<_> = tables.iter().map(|(id, table)| (*id, table)).collect();
        entries.sort_by_key(|(id, _)| id.0);
        entries
    }

    fn collect_project_used_symbols(&self) -> FxHashSet<(FileId, SymbolId)> {
        let mut used = FxHashSet::default();
        let tables = self.project_symbol_tables();
        let ordered_tables = self.ordered_table_entries(&tables);

        for (&file_id, source) in self.sources.iter() {
            let parsed = parse(source);
            let root = parsed.syntax();
            let mut symbols = tables
                .get(&file_id)
                .map(|table| (**table).clone())
                .unwrap_or_default();

            let mut importer = SymbolImporter::new(&mut symbols, &tables);
            for (other_id, table) in ordered_tables.iter().copied() {
                if other_id == file_id {
                    continue;
                }
                importer.import_table(other_id, table);
            }

            let used_ids = collect_used_symbols(&symbols, &root);
            for symbol_id in used_ids {
                let Some(symbol) = symbols.get(symbol_id) else {
                    continue;
                };
                let key = if let Some(origin) = symbol.origin {
                    (origin.file_id, origin.symbol_id)
                } else {
                    (file_id, symbol_id)
                };
                used.insert(key);
            }
        }

        used
    }

    /// Returns all known file IDs.
    pub fn file_ids(&self) -> Vec<FileId> {
        self.sources.keys().copied().collect()
    }

    /// Remove source text and cached diagnostics/symbols for a file.
    pub fn remove_source_text(&mut self, file_id: FileId) {
        self.sources.remove(&file_id);
        self.invalidate(file_id);
    }

    fn merge_project_symbols(&self, file_id: FileId, symbols: &mut SymbolTable) {
        let tables = self.project_symbol_tables();
        let ordered_tables = self.ordered_table_entries(&tables);
        let mut importer = SymbolImporter::new(symbols, &tables);
        for (other_id, table) in ordered_tables.iter().copied() {
            if other_id == file_id {
                continue;
            }
            importer.import_table(other_id, table);
        }
    }

    fn merge_project_symbols_filtered(
        &self,
        file_id: FileId,
        symbols: &mut SymbolTable,
        allowed_files: &FxHashSet<FileId>,
    ) {
        if allowed_files.is_empty() {
            return;
        }
        let tables = self.project_symbol_tables();
        let mut ordered_ids: Vec<FileId> = allowed_files.iter().copied().collect();
        ordered_ids.sort_by_key(|id| id.0);
        let mut importer = SymbolImporter::new(symbols, &tables);
        for other_id in ordered_ids {
            if other_id == file_id {
                continue;
            }
            let Some(table) = tables.get(&other_id) else {
                continue;
            };
            importer.import_table(other_id, table);
        }
    }

    fn phase_collect_symbols(
        &self,
        root: &SyntaxNode,
    ) -> (SymbolTable, Vec<Diagnostic>, Vec<PendingType>) {
        SymbolCollector::new().collect_for_project(root)
    }

    fn phase_cross_file_diagnostics(
        &self,
        root: &SyntaxNode,
        symbols: &mut SymbolTable,
        pending_types: Vec<PendingType>,
        file_id: FileId,
    ) -> Vec<Diagnostic> {
        let mut builder = DiagnosticBuilder::new();
        resolve_pending_types_with_table(symbols, pending_types, &mut builder);
        resolve_declared_var_types_with_project(symbols, root);
        check_global_external_links_with_project(symbols, root, &mut builder, file_id);
        builder.finish()
    }

    fn phase_oop_diagnostics(&self, symbols: &SymbolTable, root: &SyntaxNode) -> Vec<Diagnostic> {
        let mut builder = DiagnosticBuilder::new();
        check_class_semantics(symbols, root, &mut builder);
        check_abstract_instantiations(symbols, root, &mut builder);
        check_extends_implements_semantics(symbols, root, &mut builder);
        check_interface_conformance(symbols, root, &mut builder);
        check_property_accessors(symbols, &mut builder);
        builder.finish()
    }

    fn phase_using_diagnostics(&self, symbols: &SymbolTable) -> Vec<Diagnostic> {
        let mut builder = DiagnosticBuilder::new();
        check_using_directives(symbols, &mut builder);
        builder.finish()
    }

    fn phase_configuration_diagnostics(
        &self,
        symbols: &SymbolTable,
        root: &SyntaxNode,
    ) -> Vec<Diagnostic> {
        let mut builder = DiagnosticBuilder::new();
        check_configuration_semantics(symbols, root, &mut builder);
        builder.finish()
    }

    fn phase_type_check_diagnostics(
        &self,
        symbols: &mut SymbolTable,
        root: &SyntaxNode,
        file_id: FileId,
        project_used: &FxHashSet<(FileId, SymbolId)>,
    ) -> Vec<Diagnostic> {
        let mut builder = DiagnosticBuilder::new();
        type_check_file(symbols, root, &mut builder);
        check_unreachable_statements(root, &mut builder);
        check_cyclomatic_complexity(root, &mut builder);
        check_nondeterminism(symbols, &mut builder);
        check_shared_global_task_hazards(symbols, &self.sources, file_id, &mut builder);
        add_unused_symbol_warnings(symbols, file_id, project_used, &mut builder);
        builder.finish()
    }

    /// Returns a symbol table augmented with project-wide symbols.
    pub fn file_symbols_with_project(&self, file_id: FileId) -> Arc<SymbolTable> {
        self.analyze(file_id).symbols.clone()
    }

    /// Returns a symbol table augmented with project symbols filtered to a file set.
    pub fn file_symbols_with_project_filtered(
        &self,
        file_id: FileId,
        allowed_files: &FxHashSet<FileId>,
    ) -> Arc<SymbolTable> {
        let base = self.file_symbols(file_id);
        let mut symbols = (*base).clone();
        self.merge_project_symbols_filtered(file_id, &mut symbols, allowed_files);
        Arc::new(symbols)
    }
}

impl SourceDatabase for Database {
    fn source_text(&self, file_id: FileId) -> Arc<String> {
        self.sources
            .get(&file_id)
            .cloned()
            .unwrap_or_else(|| Arc::new(String::new()))
    }

    fn set_source_text(&mut self, file_id: FileId, text: String) {
        self.invalidate(file_id);
        self.sources.insert(file_id, Arc::new(text));
    }
}

impl SemanticDatabase for Database {
    fn file_symbols(&self, file_id: FileId) -> Arc<SymbolTable> {
        let revision = *self.revision.read().expect("revision lock poisoned");
        if let Some(entry) = self
            .symbol_cache
            .read()
            .expect("symbol cache poisoned")
            .get(&file_id)
        {
            if entry.revision == revision {
                return entry.value.clone();
            }
        }
        let source = self.source_text(file_id);
        let parsed = parse(&source);
        let root = parsed.syntax();
        let (symbols, _) = SymbolCollector::new().collect(&root);
        let symbols = Arc::new(symbols);
        self.symbol_cache
            .write()
            .expect("symbol cache poisoned")
            .insert(
                file_id,
                CacheEntry {
                    revision,
                    value: symbols.clone(),
                },
            );
        symbols
    }

    fn resolve_name(&self, file_id: FileId, name: &str) -> Option<SymbolId> {
        let symbols = self.file_symbols(file_id);
        symbols.lookup_any(name)
    }

    fn type_of(&self, file_id: FileId, expr_id: u32) -> TypeId {
        let source = self.source_text(file_id);
        let parsed = parse(&source);
        let root = parsed.syntax();
        let Some(expr_node) = expression_by_id(&root, expr_id) else {
            return TypeId::UNKNOWN;
        };

        let base_symbols = self.file_symbols(file_id);
        let mut symbols = (*base_symbols).clone();
        let context = expression_context(&symbols, &expr_node);
        self.merge_project_symbols(file_id, &mut symbols);
        resolve_declared_var_types_with_project(&mut symbols, &root);
        let symbol_hash = hash_symbol_table(&symbols);
        let expr_hash = hash_expression(&expr_node, &source);
        let cache_key = ExprCacheKey {
            scope_id: context.scope_id,
            expr_hash,
        };

        if let Some(entry) = self
            .expr_cache
            .read()
            .expect("expression cache poisoned")
            .get(&file_id)
        {
            if entry.symbol_hash == symbol_hash {
                if let Some(result) = entry.entries.get(&cache_key) {
                    return *result;
                }
            }
        }

        let mut diagnostics = DiagnosticBuilder::new();
        let mut checker = TypeChecker::new(&mut symbols, &mut diagnostics, context.scope_id);
        checker.set_return_type(context.return_type);
        checker.set_receiver_types(context.this_type, context.super_type);
        let result = checker.expr().check_expression(&expr_node);

        let mut cache = self.expr_cache.write().expect("expression cache poisoned");
        let entry = cache.entry(file_id).or_insert_with(|| ExprTypeCache {
            symbol_hash,
            entries: FxHashMap::default(),
        });
        if entry.symbol_hash != symbol_hash {
            entry.symbol_hash = symbol_hash;
            entry.entries.clear();
        }
        entry.entries.insert(cache_key, result);

        result
    }

    fn expr_id_at_offset(&self, file_id: FileId, offset: u32) -> Option<u32> {
        let source = self.source_text(file_id);
        let parsed = parse(&source);
        let root = parsed.syntax();

        let offset = TextSize::from(offset);
        expression_id_at_offset(&root, offset)
    }

    fn diagnostics(&self, file_id: FileId) -> Arc<Vec<Diagnostic>> {
        self.analyze(file_id).diagnostics.clone()
    }

    fn analyze(&self, file_id: FileId) -> Arc<FileAnalysis> {
        let revision = *self.revision.read().expect("revision lock poisoned");
        if let Some(entry) = self
            .analysis_cache
            .read()
            .expect("analysis cache poisoned")
            .get(&file_id)
        {
            if entry.revision == revision {
                return entry.value.clone();
            }
        }

        let source = self.source_text(file_id);
        let parsed = parse(&source);
        let root = parsed.syntax();

        // Collect symbols and initial diagnostics without project-only checks
        let (mut symbols, mut diagnostics, pending_types) = self.phase_collect_symbols(&root);
        self.merge_project_symbols(file_id, &mut symbols);

        diagnostics.extend(self.phase_cross_file_diagnostics(
            &root,
            &mut symbols,
            pending_types,
            file_id,
        ));
        diagnostics.extend(self.phase_oop_diagnostics(&symbols, &root));
        diagnostics.extend(self.phase_using_diagnostics(&symbols));
        diagnostics.extend(self.phase_configuration_diagnostics(&symbols, &root));
        let project_used = self.collect_project_used_symbols();
        diagnostics.extend(self.phase_type_check_diagnostics(
            &mut symbols,
            &root,
            file_id,
            &project_used,
        ));

        let analysis = Arc::new(FileAnalysis {
            symbols: Arc::new(symbols),
            diagnostics: Arc::new(diagnostics),
        });

        self.analysis_cache
            .write()
            .expect("analysis cache poisoned")
            .insert(
                file_id,
                CacheEntry {
                    revision,
                    value: analysis.clone(),
                },
            );

        analysis
    }
}

fn hash_expression(node: &SyntaxNode, source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    node.kind().hash(&mut hasher);
    let range = node.text_range();
    let start = u32::from(range.start()) as usize;
    let end = u32::from(range.end()) as usize;
    if let Some(slice) = source.get(start..end) {
        slice.hash(&mut hasher);
    } else {
        range.start().hash(&mut hasher);
        range.end().hash(&mut hasher);
    }
    hasher.finish()
}

fn hash_symbol_table(symbols: &SymbolTable) -> u64 {
    let mut hasher = DefaultHasher::new();

    let mut symbol_list: Vec<&Symbol> = symbols.iter().collect();
    symbol_list.sort_by_key(|symbol| symbol.id.0);
    symbol_list.len().hash(&mut hasher);
    for symbol in symbol_list {
        symbol.id.0.hash(&mut hasher);
        symbol.name.hash(&mut hasher);
        hash_symbol_kind(&symbol.kind, &mut hasher);
        symbol.type_id.0.hash(&mut hasher);
        symbol.direct_address.hash(&mut hasher);
        hash_visibility(symbol.visibility, &mut hasher);
        hash_modifiers(symbol.modifiers, &mut hasher);
        symbol.parent.map(|id| id.0).hash(&mut hasher);
        symbol
            .origin
            .map(|origin| (origin.file_id, origin.symbol_id))
            .hash(&mut hasher);
        if let Some(extends) = symbols.extends_name(symbol.id) {
            extends.hash(&mut hasher);
        }
        if let Some(implements) = symbols.implements_names(symbol.id) {
            implements.len().hash(&mut hasher);
            for name in implements {
                name.hash(&mut hasher);
            }
        }
    }

    let scopes = symbols.scopes();
    scopes.len().hash(&mut hasher);
    for scope in scopes {
        scope.id.0.hash(&mut hasher);
        hash_scope_kind(scope.kind, &mut hasher);
        scope.parent.map(|id| id.0).hash(&mut hasher);
        scope.owner.map(|id| id.0).hash(&mut hasher);

        let mut ids: Vec<u32> = scope.symbol_ids().map(|id| id.0).collect();
        ids.sort_unstable();
        ids.len().hash(&mut hasher);
        for id in ids {
            id.hash(&mut hasher);
        }

        scope.using_directives.len().hash(&mut hasher);
        for directive in &scope.using_directives {
            directive.path.len().hash(&mut hasher);
            for part in &directive.path {
                part.hash(&mut hasher);
            }
        }
    }

    let mut types: Vec<(&TypeId, &Type)> = symbols.types_iter().collect();
    types.sort_by_key(|(id, _)| id.0);
    types.len().hash(&mut hasher);
    for (id, ty) in types {
        id.0.hash(&mut hasher);
        hash_type(ty, &mut hasher);
    }

    hasher.finish()
}

fn hash_symbol_kind(kind: &SymbolKind, hasher: &mut DefaultHasher) {
    std::mem::discriminant(kind).hash(hasher);
    match kind {
        SymbolKind::Function {
            return_type,
            parameters,
        } => {
            return_type.0.hash(hasher);
            parameters.len().hash(hasher);
            for param in parameters {
                param.0.hash(hasher);
            }
        }
        SymbolKind::Method {
            return_type,
            parameters,
        } => {
            return_type.hash(hasher);
            parameters.len().hash(hasher);
            for param in parameters {
                param.0.hash(hasher);
            }
        }
        SymbolKind::Property {
            prop_type,
            has_get,
            has_set,
        } => {
            prop_type.0.hash(hasher);
            has_get.hash(hasher);
            has_set.hash(hasher);
        }
        SymbolKind::Variable { qualifier } => {
            hash_var_qualifier(*qualifier, hasher);
        }
        SymbolKind::EnumValue { value } => {
            value.hash(hasher);
        }
        SymbolKind::Parameter { direction } => {
            hash_param_direction(*direction, hasher);
        }
        _ => {}
    }
}

fn hash_var_qualifier(qualifier: VarQualifier, hasher: &mut DefaultHasher) {
    std::mem::discriminant(&qualifier).hash(hasher);
}

fn hash_param_direction(direction: ParamDirection, hasher: &mut DefaultHasher) {
    std::mem::discriminant(&direction).hash(hasher);
}

fn hash_scope_kind(kind: ScopeKind, hasher: &mut DefaultHasher) {
    std::mem::discriminant(&kind).hash(hasher);
}

fn hash_visibility(visibility: Visibility, hasher: &mut DefaultHasher) {
    std::mem::discriminant(&visibility).hash(hasher);
}

fn hash_modifiers(modifiers: SymbolModifiers, hasher: &mut DefaultHasher) {
    modifiers.is_final.hash(hasher);
    modifiers.is_abstract.hash(hasher);
    modifiers.is_override.hash(hasher);
}

fn hash_type(ty: &Type, hasher: &mut DefaultHasher) {
    std::mem::discriminant(ty).hash(hasher);
    match ty {
        Type::String { max_len } | Type::WString { max_len } => {
            max_len.hash(hasher);
        }
        Type::Array {
            element,
            dimensions,
        } => {
            element.0.hash(hasher);
            dimensions.len().hash(hasher);
            for (lower, upper) in dimensions {
                lower.hash(hasher);
                upper.hash(hasher);
            }
        }
        Type::Struct { name, fields } => {
            name.hash(hasher);
            fields.len().hash(hasher);
            for field in fields {
                field.name.hash(hasher);
                field.type_id.0.hash(hasher);
                field.address.hash(hasher);
            }
        }
        Type::Union { name, variants } => {
            name.hash(hasher);
            variants.len().hash(hasher);
            for variant in variants {
                variant.name.hash(hasher);
                variant.type_id.0.hash(hasher);
                variant.address.hash(hasher);
            }
        }
        Type::Enum { name, base, values } => {
            name.hash(hasher);
            base.0.hash(hasher);
            values.len().hash(hasher);
            for (value_name, value) in values {
                value_name.hash(hasher);
                value.hash(hasher);
            }
        }
        Type::Pointer { target } | Type::Reference { target } => {
            target.0.hash(hasher);
        }
        Type::Subrange { base, lower, upper } => {
            base.0.hash(hasher);
            lower.hash(hasher);
            upper.hash(hasher);
        }
        Type::FunctionBlock { name } | Type::Class { name } | Type::Interface { name } => {
            name.hash(hasher);
        }
        Type::Alias { name, target } => {
            name.hash(hasher);
            target.0.hash(hasher);
        }
        _ => {}
    }
}
