use indexmap::IndexMap;
use smol_str::SmolStr;

use crate::io::IoAddress;
use crate::task::ProgramDef;
use crate::value::Duration;
use trust_syntax::syntax::{SyntaxKind, SyntaxNode};

use super::super::lower::{const_duration_from_node, const_int_from_node, lower_expr};
use super::super::types::CompileError;
use super::super::util::{
    collect_using_directives, extract_name_from_expr, is_expression_kind, node_text,
};
use super::lower_type_ref;
use super::model::{
    AccessDecl, AccessPart, AccessPath, ConfigInit, ConfigModel, FbTaskBinding, GlobalInit,
    LoweringContext, ProgramInstanceConfig,
};
use super::vars::{parse_var_decl, var_block_kind, var_block_qualifiers, VarBlockKind};

pub(crate) fn lower_configuration(
    syntax: &SyntaxNode,
    registry: &mut trust_hir::types::TypeRegistry,
    profile: crate::value::DateTimeProfile,
    file_id: u32,
    statement_locations: &mut Vec<crate::debug::SourceLocation>,
) -> Result<Option<ConfigModel>, CompileError> {
    let configs: Vec<SyntaxNode> = syntax
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::Configuration)
        .collect();
    if configs.is_empty() {
        return Ok(None);
    }
    if configs.len() > 1 {
        return Err(CompileError::new(
            "multiple CONFIGURATION declarations not supported",
        ));
    }
    let config = configs[0].clone();
    let using = collect_using_directives(&config);
    let mut ctx = LoweringContext {
        registry,
        profile,
        using,
        file_id,
        statement_locations,
    };
    let mut globals = Vec::new();
    let mut tasks = Vec::new();
    let mut programs = Vec::new();
    let mut access = Vec::new();
    let mut config_inits = Vec::new();

    for child in config.children() {
        match child.kind() {
            SyntaxKind::VarBlock => globals.extend(lower_global_var_block(&child, &mut ctx)?),
            SyntaxKind::TaskConfig => tasks.push(lower_task_config(&child, &mut ctx)?),
            SyntaxKind::ProgramConfig => programs.push(lower_program_config(&child, &mut ctx)?),
            SyntaxKind::VarAccessBlock => {
                let result = lower_var_access_block(&child, &mut ctx)?;
                globals.extend(result.globals);
                access.extend(result.access);
            }
            SyntaxKind::VarConfigBlock => {
                config_inits.extend(lower_var_config_block(&child, &mut ctx)?);
            }
            SyntaxKind::Resource => {
                let resource = child;
                for res_child in resource.children() {
                    match res_child.kind() {
                        SyntaxKind::VarBlock => {
                            globals.extend(lower_global_var_block(&res_child, &mut ctx)?)
                        }
                        SyntaxKind::TaskConfig => {
                            tasks.push(lower_task_config(&res_child, &mut ctx)?)
                        }
                        SyntaxKind::ProgramConfig => {
                            programs.push(lower_program_config(&res_child, &mut ctx)?)
                        }
                        SyntaxKind::VarAccessBlock => {
                            let result = lower_var_access_block(&res_child, &mut ctx)?;
                            globals.extend(result.globals);
                            access.extend(result.access);
                        }
                        SyntaxKind::VarConfigBlock => {
                            config_inits.extend(lower_var_config_block(&res_child, &mut ctx)?);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Some(ConfigModel {
        globals,
        tasks,
        programs,
        using: ctx.using.clone(),
        access,
        config_inits,
    }))
}

fn lower_global_var_block(
    var_block: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<Vec<GlobalInit>, CompileError> {
    let mut globals = Vec::new();
    let kind = var_block_kind(var_block)?;
    let qualifiers = var_block_qualifiers(var_block);
    for var_decl in var_block
        .children()
        .filter(|child| child.kind() == SyntaxKind::VarDecl)
    {
        let (names, type_ref, initializer, address) = parse_var_decl(&var_decl)?;
        let type_id = lower_type_ref(&type_ref, ctx)?;
        let init_expr = initializer.map(|expr| lower_expr(&expr, ctx)).transpose()?;
        match kind {
            VarBlockKind::Global
            | VarBlockKind::Var
            | VarBlockKind::Input
            | VarBlockKind::Output
            | VarBlockKind::InOut => {
                for name in names {
                    globals.push(GlobalInit {
                        name,
                        type_id,
                        initializer: init_expr.clone(),
                        retain: qualifiers.retain,
                        address: address.clone(),
                        using: ctx.using.clone(),
                    });
                }
            }
            VarBlockKind::External => {
                continue;
            }
            _ => {
                return Err(CompileError::new(
                    "unsupported VAR block in CONFIGURATION/RESOURCE",
                ));
            }
        }
    }
    Ok(globals)
}

#[derive(Default)]
struct VarAccessResult {
    globals: Vec<GlobalInit>,
    access: Vec<AccessDecl>,
}

fn lower_var_access_block(
    var_block: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<VarAccessResult, CompileError> {
    let mut result = VarAccessResult::default();
    for access_decl in var_block
        .children()
        .filter(|child| child.kind() == SyntaxKind::AccessDecl)
    {
        let name_node = access_decl
            .children()
            .find(|child| child.kind() == SyntaxKind::Name)
            .ok_or_else(|| CompileError::new("missing VAR_ACCESS name"))?;
        let name = SmolStr::new(node_text(&name_node));
        let path_node = access_decl
            .children()
            .find(|child| child.kind() == SyntaxKind::AccessPath)
            .ok_or_else(|| CompileError::new("missing VAR_ACCESS path"))?;
        let type_ref = access_decl
            .children()
            .find(|child| child.kind() == SyntaxKind::TypeRef)
            .ok_or_else(|| CompileError::new("missing VAR_ACCESS type"))?;
        let type_id = lower_type_ref(&type_ref, ctx)?;
        let path = parse_access_path(&path_node, ctx)?;

        match &path {
            AccessPath::Direct { text, .. } => {
                result.globals.push(GlobalInit {
                    name,
                    type_id,
                    initializer: None,
                    retain: crate::RetainPolicy::Unspecified,
                    address: Some(text.clone()),
                    using: ctx.using.clone(),
                });
            }
            AccessPath::Parts(_) => {
                result.access.push(AccessDecl { name, path });
            }
        }
    }
    Ok(result)
}

fn lower_var_config_block(
    var_block: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<Vec<ConfigInit>, CompileError> {
    let mut inits = Vec::new();
    for config_init in var_block
        .children()
        .filter(|child| child.kind() == SyntaxKind::ConfigInit)
    {
        let path_node = config_init
            .children()
            .find(|child| child.kind() == SyntaxKind::AccessPath)
            .ok_or_else(|| CompileError::new("missing VAR_CONFIG path"))?;
        let type_ref = config_init
            .children()
            .find(|child| child.kind() == SyntaxKind::TypeRef)
            .ok_or_else(|| CompileError::new("missing VAR_CONFIG type"))?;
        let path = parse_access_path(&path_node, ctx)?;
        let type_id = lower_type_ref(&type_ref, ctx)?;
        let initializer = config_init
            .children()
            .find(|child| is_expression_kind(child.kind()))
            .map(|expr| lower_expr(&expr, ctx))
            .transpose()?;
        let address = config_init_address(&config_init)?;
        inits.push(ConfigInit {
            path,
            address,
            type_id,
            initializer,
        });
    }
    Ok(inits)
}

fn config_init_address(node: &SyntaxNode) -> Result<Option<IoAddress>, CompileError> {
    let mut seen_at = false;
    for element in node.children_with_tokens() {
        let token = match element.into_token() {
            Some(token) => token,
            None => continue,
        };
        match token.kind() {
            SyntaxKind::KwAt => seen_at = true,
            SyntaxKind::DirectAddress if seen_at => {
                let address = IoAddress::parse(token.text())
                    .map_err(|err| CompileError::new(err.to_string()))?;
                return Ok(Some(address));
            }
            _ if !token.kind().is_trivia() => seen_at = false,
            _ => {}
        }
    }
    Ok(None)
}

fn parse_access_path(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<AccessPath, CompileError> {
    let mut parts = Vec::new();
    let mut index_nodes: Vec<SyntaxNode> = Vec::new();
    let mut in_index = false;
    let mut saw_root = false;

    for element in node.children_with_tokens() {
        if let Some(token) = element.as_token() {
            match token.kind() {
                SyntaxKind::LBracket => {
                    in_index = true;
                    index_nodes.clear();
                    continue;
                }
                SyntaxKind::RBracket => {
                    in_index = false;
                    if index_nodes.is_empty() {
                        return Err(CompileError::new("empty array index in access path"));
                    }
                    let mut indices = Vec::new();
                    for expr in &index_nodes {
                        let value = const_int_from_node(expr, ctx)?;
                        indices.push(value);
                    }
                    parts.push(AccessPart::Index(indices));
                    index_nodes.clear();
                    continue;
                }
                SyntaxKind::DirectAddress if !saw_root => {
                    let text = SmolStr::new(token.text());
                    let address = IoAddress::parse(text.as_ref())
                        .map_err(|err| CompileError::new(err.to_string()))?;
                    return Ok(AccessPath::Direct { address, text });
                }
                SyntaxKind::DirectAddress => {
                    let text = token.text();
                    if let Some(partial) = crate::value::parse_partial_access(text) {
                        parts.push(AccessPart::Partial(partial));
                    } else {
                        return Err(CompileError::new(
                            "unexpected direct address in access path",
                        ));
                    }
                }
                SyntaxKind::IntLiteral => {
                    if let Some(partial) = crate::value::parse_partial_access(token.text()) {
                        parts.push(AccessPart::Partial(partial));
                    }
                }
                _ => {}
            }
            continue;
        }
        if let Some(child) = element.as_node() {
            if in_index {
                if is_expression_kind(child.kind()) {
                    index_nodes.push(child.clone());
                }
                continue;
            }
            if child.kind() == SyntaxKind::Name {
                let name = SmolStr::new(node_text(child));
                parts.push(AccessPart::Name(name));
                saw_root = true;
            } else if is_expression_kind(child.kind()) {
                index_nodes.push(child.clone());
            }
        }
    }

    if in_index {
        return Err(CompileError::new("unterminated array index in access path"));
    }
    if parts.is_empty() {
        return Err(CompileError::new("empty access path"));
    }
    Ok(AccessPath::Parts(parts))
}

fn lower_task_config(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<crate::task::TaskConfig, CompileError> {
    let name_node = node
        .children()
        .find(|child| child.kind() == SyntaxKind::Name)
        .ok_or_else(|| CompileError::new("missing task name"))?;
    let name = SmolStr::new(node_text(&name_node));

    let mut interval = Duration::ZERO;
    let mut single = None;
    let mut priority: u32 = 0;

    if let Some(init) = node
        .children()
        .find(|child| child.kind() == SyntaxKind::TaskInit)
    {
        let mut current_key: Option<String> = None;
        for child in init.children() {
            match child.kind() {
                SyntaxKind::Name => {
                    current_key = Some(node_text(&child));
                }
                _ if is_expression_kind(child.kind()) => {
                    if let Some(key) = current_key.take() {
                        match key.to_ascii_uppercase().as_str() {
                            "INTERVAL" => {
                                interval = const_duration_from_node(&child, ctx)?;
                            }
                            "SINGLE" => {
                                let name = extract_name_from_expr(&child).ok_or_else(|| {
                                    CompileError::new("invalid SINGLE expression")
                                })?;
                                single = Some(name);
                            }
                            "PRIORITY" => {
                                let value = const_int_from_node(&child, ctx)?;
                                priority = u32::try_from(value).map_err(|_| {
                                    CompileError::new("TASK PRIORITY must be non-negative")
                                })?;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(crate::task::TaskConfig {
        name,
        interval,
        single,
        priority,
        programs: Vec::new(),
        fb_instances: Vec::new(),
    })
}

fn lower_program_config(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<ProgramInstanceConfig, CompileError> {
    let mut retain = None;
    let mut instance = None;
    let mut task = None;
    let mut fb_tasks = Vec::new();
    let mut type_node = None;
    let mut seen_with = false;
    for element in node.children_with_tokens() {
        if let Some(child) = element.as_node() {
            match child.kind() {
                SyntaxKind::Name => {
                    let name = SmolStr::new(node_text(child));
                    if seen_with {
                        task = Some(name);
                        seen_with = false;
                    } else if instance.is_none() {
                        instance = Some(name);
                    }
                }
                SyntaxKind::QualifiedName | SyntaxKind::TypeRef => {
                    type_node = Some(child.clone());
                }
                _ => {}
            }
            continue;
        }

        let Some(token) = element.as_token() else {
            continue;
        };
        if token.kind().is_trivia() {
            continue;
        }
        match token.kind() {
            SyntaxKind::KwRetain => retain = Some(crate::RetainPolicy::Retain),
            SyntaxKind::KwNonRetain => retain = Some(crate::RetainPolicy::NonRetain),
            SyntaxKind::KwWith => seen_with = true,
            _ => {}
        }
    }

    let instance = instance.ok_or_else(|| CompileError::new("missing program instance name"))?;
    let type_node = type_node.ok_or_else(|| CompileError::new("missing program type"))?;
    let type_name = SmolStr::new(node_text(&type_node));

    if let Some(list) = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ProgramConfigList)
    {
        fb_tasks = lower_program_config_list(&list, ctx)?;
    }

    Ok(ProgramInstanceConfig {
        name: instance,
        type_name,
        task,
        retain,
        fb_tasks,
    })
}

fn lower_program_config_list(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<Vec<FbTaskBinding>, CompileError> {
    let mut bindings = Vec::new();
    for elem in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::ProgramConfigElem)
    {
        let mut seen_with = false;
        let mut task_name: Option<SmolStr> = None;
        for element in elem.children_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == SyntaxKind::KwWith {
                    seen_with = true;
                }
                continue;
            }
            let Some(child) = element.as_node() else {
                continue;
            };
            if child.kind() == SyntaxKind::Name && seen_with {
                task_name = Some(SmolStr::new(node_text(child)));
                break;
            }
        }

        if let Some(task) = task_name {
            let path_node = elem
                .children()
                .find(|child| child.kind() == SyntaxKind::AccessPath)
                .ok_or_else(|| CompileError::new("missing access path for FB task"))?;
            let path = parse_access_path(&path_node, ctx)?;
            bindings.push(FbTaskBinding { path, task });
        }
    }
    Ok(bindings)
}

pub(crate) fn resolve_program_type_name(
    program_defs: &IndexMap<SmolStr, ProgramDef>,
    type_name: &SmolStr,
    using: &[SmolStr],
) -> Result<SmolStr, CompileError> {
    let direct_key = SmolStr::new(type_name.to_ascii_uppercase());
    if let Some(def) = program_defs.get(&direct_key) {
        return Ok(def.name.clone());
    }
    if !type_name.contains('.') {
        for namespace in using {
            let qualified = format!("{namespace}.{type_name}");
            let key = SmolStr::new(qualified.to_ascii_uppercase());
            if let Some(def) = program_defs.get(&key) {
                return Ok(def.name.clone());
            }
        }
    }
    Err(CompileError::new(format!(
        "unknown PROGRAM type '{}'",
        type_name
    )))
}
