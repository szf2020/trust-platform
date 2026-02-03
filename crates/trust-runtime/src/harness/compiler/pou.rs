use smol_str::SmolStr;
use trust_hir::symbols::ParamDirection;
use trust_syntax::syntax::{SyntaxKind, SyntaxNode};

use crate::eval::{
    ClassDef, FunctionBlockBase, FunctionBlockDef, FunctionDef, InterfaceDef, MethodDef, Param,
    VarDef,
};
use crate::io::IoAddress;
use crate::task::ProgramDef;
use crate::value::DateTimeProfile;

use super::super::lower::{lower_expr, lower_stmt_list};
use super::super::types::CompileError;
use super::super::util::{collect_using_directives, node_text};
use super::model::{GlobalInit, LoweredProgram, LoweringContext, ProgramVars};
use super::types::qualify_with_namespaces;
use super::vars::{parse_var_decl, var_block_kind, var_block_qualifiers, VarBlockKind};
use super::{lower_type_ref, resolve_named_type};

pub(crate) fn lower_programs(
    syntax: &SyntaxNode,
    registry: &mut trust_hir::types::TypeRegistry,
    profile: DateTimeProfile,
    file_id: u32,
    statement_locations: &mut Vec<crate::debug::SourceLocation>,
) -> Result<Vec<LoweredProgram>, CompileError> {
    let mut programs = Vec::new();
    for program_node in syntax
        .children()
        .filter(|child| child.kind() == SyntaxKind::Program)
    {
        programs.push(lower_program_node(
            &program_node,
            registry,
            profile,
            file_id,
            statement_locations,
        )?);
    }
    Ok(programs)
}

pub(crate) fn lower_functions(
    syntax: &SyntaxNode,
    registry: &mut trust_hir::types::TypeRegistry,
    profile: DateTimeProfile,
    file_id: u32,
    statement_locations: &mut Vec<crate::debug::SourceLocation>,
) -> Result<Vec<FunctionDef>, CompileError> {
    let mut functions = Vec::new();
    for func_node in syntax
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::Function)
    {
        let using = collect_using_directives(&func_node);
        let mut ctx = LoweringContext {
            registry,
            profile,
            using,
            file_id,
            statement_locations,
        };
        functions.push(lower_function_node(&func_node, &mut ctx)?);
    }
    Ok(functions)
}

pub(crate) fn lower_function_blocks(
    syntax: &SyntaxNode,
    registry: &mut trust_hir::types::TypeRegistry,
    profile: DateTimeProfile,
    file_id: u32,
    statement_locations: &mut Vec<crate::debug::SourceLocation>,
) -> Result<Vec<FunctionBlockDef>, CompileError> {
    let mut function_blocks = Vec::new();
    for fb_node in syntax
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::FunctionBlock)
    {
        let using = collect_using_directives(&fb_node);
        let mut ctx = LoweringContext {
            registry,
            profile,
            using,
            file_id,
            statement_locations,
        };
        function_blocks.push(lower_function_block_node(&fb_node, &mut ctx)?);
    }
    Ok(function_blocks)
}

pub(crate) fn lower_classes(
    syntax: &SyntaxNode,
    registry: &mut trust_hir::types::TypeRegistry,
    profile: DateTimeProfile,
    file_id: u32,
    statement_locations: &mut Vec<crate::debug::SourceLocation>,
) -> Result<Vec<ClassDef>, CompileError> {
    let mut classes = Vec::new();
    for class_node in syntax
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::Class)
    {
        let using = collect_using_directives(&class_node);
        let mut ctx = LoweringContext {
            registry,
            profile,
            using,
            file_id,
            statement_locations,
        };
        classes.push(lower_class_node(&class_node, &mut ctx)?);
    }
    Ok(classes)
}

pub(crate) fn lower_interfaces(
    syntax: &SyntaxNode,
    registry: &mut trust_hir::types::TypeRegistry,
    profile: DateTimeProfile,
    file_id: u32,
    statement_locations: &mut Vec<crate::debug::SourceLocation>,
) -> Result<Vec<InterfaceDef>, CompileError> {
    let mut interfaces = Vec::new();
    for interface_node in syntax
        .descendants()
        .filter(|child| child.kind() == SyntaxKind::Interface)
    {
        let using = collect_using_directives(&interface_node);
        let mut ctx = LoweringContext {
            registry,
            profile,
            using,
            file_id,
            statement_locations,
        };
        interfaces.push(lower_interface_node(&interface_node, &mut ctx)?);
    }
    Ok(interfaces)
}

fn lower_program_node(
    program_node: &SyntaxNode,
    registry: &mut trust_hir::types::TypeRegistry,
    profile: DateTimeProfile,
    file_id: u32,
    statement_locations: &mut Vec<crate::debug::SourceLocation>,
) -> Result<LoweredProgram, CompileError> {
    let name = qualified_pou_name(program_node)?;
    let using = collect_using_directives(program_node);
    let mut ctx = LoweringContext {
        registry,
        profile,
        using,
        file_id,
        statement_locations,
    };
    let vars = lower_program_var_blocks(program_node, &mut ctx)?;
    let body = lower_stmt_list(program_node, &mut ctx)?;
    Ok(LoweredProgram {
        program: ProgramDef {
            name,
            vars: vars.vars,
            temps: vars.temps,
            using: ctx.using.clone(),
            body,
        },
        globals: vars.globals,
    })
}

fn lower_function_block_node(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<FunctionBlockDef, CompileError> {
    let name = qualified_pou_name(node)?;
    let mut base = None;
    if let Some(extends_clause) = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ExtendsClause)
    {
        if let Some(base_name) = extends_clause
            .children()
            .find(|child| child.kind() == SyntaxKind::Name)
        {
            let raw = node_text(&base_name);
            let resolved = resolve_named_type(ctx.registry, &raw, &ctx.using)?;
            let type_id = ctx
                .registry
                .lookup(resolved.as_ref())
                .ok_or_else(|| CompileError::new("unknown base type"))?;
            let base_type = ctx
                .registry
                .get(type_id)
                .ok_or_else(|| CompileError::new("unknown base type"))?;
            base = Some(match base_type {
                trust_hir::Type::FunctionBlock { .. } => FunctionBlockBase::FunctionBlock(resolved),
                trust_hir::Type::Class { .. } => FunctionBlockBase::Class(resolved),
                _ => {
                    return Err(CompileError::new(
                        "function block EXTENDS must reference a FUNCTION_BLOCK or CLASS",
                    ))
                }
            });
        }
    }
    let (params, vars, temps) = lower_function_block_var_blocks(node, ctx)?;
    let mut methods = Vec::new();
    for method_node in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::Method)
    {
        methods.push(lower_method_node(&method_node, ctx)?);
    }
    let body = lower_stmt_list(node, ctx)?;
    Ok(FunctionBlockDef {
        name,
        base,
        params,
        vars,
        temps,
        using: ctx.using.clone(),
        methods,
        body,
    })
}

fn lower_class_node(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<ClassDef, CompileError> {
    let name = qualified_pou_name(node)?;
    let mut base = None;
    if let Some(extends_clause) = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ExtendsClause)
    {
        if let Some(base_name) = extends_clause
            .children()
            .find(|child| child.kind() == SyntaxKind::Name)
        {
            let raw = node_text(&base_name);
            base = Some(resolve_named_type(ctx.registry, &raw, &ctx.using)?);
        }
    }

    let vars = lower_class_var_blocks(node, ctx)?;
    let mut methods = Vec::new();
    for method_node in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::Method)
    {
        methods.push(lower_method_node(&method_node, ctx)?);
    }

    Ok(ClassDef {
        name,
        base,
        vars,
        using: ctx.using.clone(),
        methods,
    })
}

fn lower_interface_node(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<InterfaceDef, CompileError> {
    let name = qualified_pou_name(node)?;
    let mut base = None;
    if let Some(extends_clause) = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ExtendsClause)
    {
        if let Some(base_name) = extends_clause
            .children()
            .find(|child| child.kind() == SyntaxKind::Name)
        {
            let raw = node_text(&base_name);
            base = Some(resolve_named_type(ctx.registry, &raw, &ctx.using)?);
        }
    }

    let mut methods = Vec::new();
    for method_node in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::Method)
    {
        methods.push(lower_method_node(&method_node, ctx)?);
    }

    Ok(InterfaceDef {
        name,
        base,
        using: ctx.using.clone(),
        methods,
    })
}

fn lower_function_node(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<FunctionDef, CompileError> {
    let name = qualified_pou_name(node)?;
    let return_type = node
        .children()
        .find(|child| child.kind() == SyntaxKind::TypeRef)
        .ok_or_else(|| CompileError::new("missing function return type"))?;
    let return_type = lower_type_ref(&return_type, ctx)?;

    let (params, locals) = lower_function_var_blocks(node, ctx)?;
    let body = lower_stmt_list(node, ctx)?;

    Ok(FunctionDef {
        name,
        return_type,
        params,
        locals,
        using: ctx.using.clone(),
        body,
    })
}

fn lower_method_node(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<MethodDef, CompileError> {
    let name_node = node
        .children()
        .find(|child| child.kind() == SyntaxKind::Name)
        .ok_or_else(|| CompileError::new("missing method name"))?;
    let raw = node_text(&name_node);
    let name = qualify_with_namespaces(node, &raw);

    let using = collect_using_directives(node);
    let mut method_ctx = LoweringContext {
        registry: ctx.registry,
        profile: ctx.profile,
        using,
        file_id: ctx.file_id,
        statement_locations: ctx.statement_locations,
    };

    let return_type = node
        .children()
        .find(|child| child.kind() == SyntaxKind::TypeRef)
        .map(|type_ref| lower_type_ref(&type_ref, &mut method_ctx))
        .transpose()?;

    let (params, locals) = lower_function_var_blocks(node, &mut method_ctx)?;
    let body = lower_stmt_list(node, &mut method_ctx)?;

    Ok(MethodDef {
        name,
        return_type,
        params,
        locals,
        using: method_ctx.using.clone(),
        body,
    })
}

pub(crate) fn qualified_pou_name(node: &SyntaxNode) -> Result<SmolStr, CompileError> {
    let name_node = node
        .children()
        .find(|child| child.kind() == SyntaxKind::Name)
        .ok_or_else(|| CompileError::new("missing POU name"))?;
    let mut parts = Vec::new();
    parts.push(node_text(&name_node));
    for ancestor in node.ancestors() {
        if ancestor.kind() != SyntaxKind::Namespace {
            continue;
        }
        if let Some(ns_name) = ancestor
            .children()
            .find(|child| child.kind() == SyntaxKind::Name)
        {
            parts.push(node_text(&ns_name));
        }
    }
    parts.reverse();
    Ok(parts.join(".").into())
}

fn lower_program_var_blocks(
    program: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<ProgramVars, CompileError> {
    let mut globals = Vec::new();
    let mut vars = Vec::new();
    let mut temps = Vec::new();
    for var_block in program
        .children()
        .filter(|child| child.kind() == SyntaxKind::VarBlock)
    {
        let kind = var_block_kind(&var_block)?;
        let qualifiers = var_block_qualifiers(&var_block);
        for var_decl in var_block
            .children()
            .filter(|child| child.kind() == SyntaxKind::VarDecl)
        {
            let (names, type_ref, initializer, address) = parse_var_decl(&var_decl)?;
            let type_id = lower_type_ref(&type_ref, ctx)?;
            let init_expr = initializer.map(|expr| lower_expr(&expr, ctx)).transpose()?;
            let address_info = address
                .as_ref()
                .map(|text| IoAddress::parse(text))
                .transpose()
                .map_err(|err| CompileError::new(format!("invalid I/O address: {err}")))?;
            if matches!(kind, VarBlockKind::Input | VarBlockKind::InOut)
                && address_info
                    .as_ref()
                    .map(|addr| addr.wildcard)
                    .unwrap_or(false)
            {
                return Err(CompileError::new(
                    "wildcard address not allowed in VAR_INPUT/VAR_IN_OUT",
                ));
            }
            match kind {
                VarBlockKind::Temp => {
                    for name in names {
                        temps.push(VarDef {
                            name,
                            type_id,
                            initializer: init_expr.clone(),
                            retain: qualifiers.retain,
                            external: false,
                            constant: qualifiers.constant,
                            address: address_info.clone(),
                        });
                    }
                }
                VarBlockKind::External => {
                    continue;
                }
                VarBlockKind::Global => {
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
                VarBlockKind::Input
                | VarBlockKind::Output
                | VarBlockKind::InOut
                | VarBlockKind::Var => {
                    for name in names {
                        vars.push(VarDef {
                            name,
                            type_id,
                            initializer: init_expr.clone(),
                            retain: qualifiers.retain,
                            external: false,
                            constant: qualifiers.constant,
                            address: address_info.clone(),
                        });
                    }
                }
                VarBlockKind::Unsupported => {
                    return Err(CompileError::new("unsupported VAR block in PROGRAM"));
                }
            }
        }
    }
    Ok(ProgramVars {
        globals,
        vars,
        temps,
    })
}

type FunctionBlockVars = (Vec<Param>, Vec<VarDef>, Vec<VarDef>);

fn lower_function_var_blocks(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<(Vec<Param>, Vec<VarDef>), CompileError> {
    let mut params = Vec::new();
    let mut locals = Vec::new();
    for var_block in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::VarBlock)
    {
        let kind = var_block_kind(&var_block)?;
        let qualifiers = var_block_qualifiers(&var_block);
        for var_decl in var_block
            .children()
            .filter(|child| child.kind() == SyntaxKind::VarDecl)
        {
            let (names, type_ref, initializer, address) = parse_var_decl(&var_decl)?;
            let type_id = lower_type_ref(&type_ref, ctx)?;
            let init_expr = initializer.map(|expr| lower_expr(&expr, ctx)).transpose()?;
            let address_info = address
                .as_ref()
                .map(|text| IoAddress::parse(text))
                .transpose()
                .map_err(|err| CompileError::new(format!("invalid I/O address: {err}")))?;
            if matches!(kind, VarBlockKind::Input | VarBlockKind::InOut)
                && address_info
                    .as_ref()
                    .map(|addr| addr.wildcard)
                    .unwrap_or(false)
            {
                return Err(CompileError::new(
                    "wildcard address not allowed in VAR_INPUT/VAR_IN_OUT",
                ));
            }
            match kind {
                VarBlockKind::Input => {
                    for name in names {
                        params.push(Param {
                            name,
                            type_id,
                            direction: ParamDirection::In,
                            address: address_info.clone(),
                            default: init_expr.clone(),
                        });
                    }
                }
                VarBlockKind::Output => {
                    for name in names {
                        params.push(Param {
                            name,
                            type_id,
                            direction: ParamDirection::Out,
                            address: address_info.clone(),
                            default: None,
                        });
                    }
                }
                VarBlockKind::InOut => {
                    for name in names {
                        params.push(Param {
                            name,
                            type_id,
                            direction: ParamDirection::InOut,
                            address: address_info.clone(),
                            default: None,
                        });
                    }
                }
                VarBlockKind::Var | VarBlockKind::Temp => {
                    for name in names {
                        locals.push(VarDef {
                            name,
                            type_id,
                            initializer: init_expr.clone(),
                            retain: qualifiers.retain,
                            external: false,
                            constant: qualifiers.constant,
                            address: address_info.clone(),
                        });
                    }
                }
                VarBlockKind::External => {
                    continue;
                }
                VarBlockKind::Global | VarBlockKind::Unsupported => {
                    return Err(CompileError::new(
                        "unsupported VAR block in function or function block",
                    ));
                }
            }
        }
    }
    Ok((params, locals))
}

fn lower_class_var_blocks(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<Vec<VarDef>, CompileError> {
    let mut vars = Vec::new();
    for var_block in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::VarBlock)
    {
        let kind = var_block_kind(&var_block)?;
        let qualifiers = var_block_qualifiers(&var_block);
        for var_decl in var_block
            .children()
            .filter(|child| child.kind() == SyntaxKind::VarDecl)
        {
            let (names, type_ref, initializer, address) = parse_var_decl(&var_decl)?;
            let type_id = lower_type_ref(&type_ref, ctx)?;
            let init_expr = initializer.map(|expr| lower_expr(&expr, ctx)).transpose()?;
            let address_info = address
                .as_ref()
                .map(|text| IoAddress::parse(text))
                .transpose()
                .map_err(|err| CompileError::new(format!("invalid I/O address: {err}")))?;
            if matches!(kind, VarBlockKind::Input | VarBlockKind::InOut)
                && address_info
                    .as_ref()
                    .map(|addr| addr.wildcard)
                    .unwrap_or(false)
            {
                return Err(CompileError::new(
                    "wildcard address not allowed in VAR_INPUT/VAR_IN_OUT",
                ));
            }
            match kind {
                VarBlockKind::Var
                | VarBlockKind::Input
                | VarBlockKind::Output
                | VarBlockKind::InOut => {
                    for name in names {
                        vars.push(VarDef {
                            name,
                            type_id,
                            initializer: init_expr.clone(),
                            retain: qualifiers.retain,
                            external: false,
                            constant: qualifiers.constant,
                            address: address_info.clone(),
                        });
                    }
                }
                VarBlockKind::External => {
                    continue;
                }
                _ => {
                    return Err(CompileError::new("unsupported VAR block in CLASS"));
                }
            }
        }
    }
    Ok(vars)
}

fn lower_function_block_var_blocks(
    node: &SyntaxNode,
    ctx: &mut LoweringContext<'_>,
) -> Result<FunctionBlockVars, CompileError> {
    let mut params = Vec::new();
    let mut vars = Vec::new();
    let mut temps = Vec::new();
    for var_block in node
        .children()
        .filter(|child| child.kind() == SyntaxKind::VarBlock)
    {
        let kind = var_block_kind(&var_block)?;
        let qualifiers = var_block_qualifiers(&var_block);
        for var_decl in var_block
            .children()
            .filter(|child| child.kind() == SyntaxKind::VarDecl)
        {
            let (names, type_ref, initializer, address) = parse_var_decl(&var_decl)?;
            let type_id = lower_type_ref(&type_ref, ctx)?;
            let init_expr = initializer.map(|expr| lower_expr(&expr, ctx)).transpose()?;
            let address_info = address
                .as_ref()
                .map(|text| IoAddress::parse(text))
                .transpose()
                .map_err(|err| CompileError::new(format!("invalid I/O address: {err}")))?;
            if matches!(kind, VarBlockKind::Input | VarBlockKind::InOut)
                && address_info
                    .as_ref()
                    .map(|addr| addr.wildcard)
                    .unwrap_or(false)
            {
                return Err(CompileError::new(
                    "wildcard address not allowed in VAR_INPUT/VAR_IN_OUT",
                ));
            }
            match kind {
                VarBlockKind::Input => {
                    for name in names {
                        params.push(Param {
                            name,
                            type_id,
                            direction: ParamDirection::In,
                            address: address_info.clone(),
                            default: init_expr.clone(),
                        });
                    }
                }
                VarBlockKind::Output => {
                    for name in names {
                        params.push(Param {
                            name,
                            type_id,
                            direction: ParamDirection::Out,
                            address: address_info.clone(),
                            default: None,
                        });
                    }
                }
                VarBlockKind::InOut => {
                    for name in names {
                        params.push(Param {
                            name,
                            type_id,
                            direction: ParamDirection::InOut,
                            address: address_info.clone(),
                            default: None,
                        });
                    }
                }
                VarBlockKind::Var => {
                    for name in names {
                        vars.push(VarDef {
                            name,
                            type_id,
                            initializer: init_expr.clone(),
                            retain: qualifiers.retain,
                            external: false,
                            constant: qualifiers.constant,
                            address: address_info.clone(),
                        });
                    }
                }
                VarBlockKind::Temp => {
                    for name in names {
                        temps.push(VarDef {
                            name,
                            type_id,
                            initializer: init_expr.clone(),
                            retain: qualifiers.retain,
                            external: false,
                            constant: qualifiers.constant,
                            address: address_info.clone(),
                        });
                    }
                }
                VarBlockKind::External => {
                    continue;
                }
                VarBlockKind::Global | VarBlockKind::Unsupported => {
                    return Err(CompileError::new("unsupported VAR block in function block"));
                }
            }
        }
    }
    Ok((params, vars, temps))
}
