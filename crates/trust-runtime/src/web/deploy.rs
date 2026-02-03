//! Bundle deploy helpers for the web UI.

#![allow(missing_docs)]

use std::fs;
use std::path::{Component, Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;

use crate::error::RuntimeError;

#[derive(Debug, Deserialize)]
pub struct DeployRequest {
    pub runtime_toml: Option<String>,
    pub io_toml: Option<String>,
    pub program_stbc_b64: Option<String>,
    pub sources: Option<Vec<DeploySource>>,
    pub restart: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeploySource {
    pub path: String,
    pub content: String,
}

#[derive(Debug)]
pub struct DeployResult {
    pub written: Vec<String>,
    pub restart: Option<String>,
}

#[derive(Debug)]
pub struct RollbackResult {
    pub current: PathBuf,
    pub previous: PathBuf,
}

pub fn apply_deploy(
    bundle_root: &Path,
    request: DeployRequest,
) -> Result<DeployResult, RuntimeError> {
    if !bundle_root.is_dir() {
        return Err(RuntimeError::ControlError(
            format!("project folder not found: {}", bundle_root.display()).into(),
        ));
    }
    let mut written = Vec::new();
    if let Some(runtime_toml) = request.runtime_toml {
        let path = bundle_root.join("runtime.toml");
        fs::write(&path, runtime_toml).map_err(|err| {
            RuntimeError::ControlError(format!("write runtime.toml: {err}").into())
        })?;
        written.push("runtime.toml".to_string());
    }
    if let Some(io_toml) = request.io_toml {
        let path = bundle_root.join("io.toml");
        fs::write(&path, io_toml)
            .map_err(|err| RuntimeError::ControlError(format!("write io.toml: {err}").into()))?;
        written.push("io.toml".to_string());
    }
    if let Some(program_b64) = request.program_stbc_b64 {
        let bytes = STANDARD.decode(program_b64.trim()).map_err(|err| {
            RuntimeError::ControlError(format!("decode program.stbc: {err}").into())
        })?;
        let path = bundle_root.join("program.stbc");
        fs::write(&path, bytes).map_err(|err| {
            RuntimeError::ControlError(format!("write program.stbc: {err}").into())
        })?;
        written.push("program.stbc".to_string());
    }
    if let Some(sources) = request.sources {
        let sources_root = bundle_root.join("sources");
        for source in sources {
            let rel = sanitize_relative_path(&source.path).ok_or_else(|| {
                RuntimeError::ControlError(format!("invalid source path: {}", source.path).into())
            })?;
            let dest = sources_root.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    RuntimeError::ControlError(format!("create sources dir: {err}").into())
                })?;
            }
            fs::write(&dest, source.content).map_err(|err| {
                RuntimeError::ControlError(format!("write source {}: {err}", dest.display()).into())
            })?;
            written.push(format!("sources/{}", source.path));
        }
    }
    if written.is_empty() {
        return Err(RuntimeError::ControlError(
            "no deploy payload provided".into(),
        ));
    }
    Ok(DeployResult {
        written,
        restart: request.restart,
    })
}

pub fn apply_rollback(root: &Path) -> Result<RollbackResult, RuntimeError> {
    let current_link = root.join("current");
    let previous_link = root.join("previous");
    let current_target = read_link_target(&current_link).ok_or_else(|| {
        RuntimeError::ControlError(
            format!("no current project link at {}", current_link.display()).into(),
        )
    })?;
    let previous_target = read_link_target(&previous_link).ok_or_else(|| {
        RuntimeError::ControlError(
            format!("no previous project link at {}", previous_link.display()).into(),
        )
    })?;
    update_symlink(&current_link, &previous_target)?;
    update_symlink(&previous_link, &current_target)?;
    Ok(RollbackResult {
        current: previous_target,
        previous: current_target,
    })
}

fn read_link_target(path: &Path) -> Option<PathBuf> {
    std::fs::read_link(path).ok()
}

fn update_symlink(link: &Path, target: &Path) -> Result<(), RuntimeError> {
    if link.exists() {
        std::fs::remove_file(link).map_err(|err| {
            RuntimeError::ControlError(format!("remove link {}: {err}", link.display()).into())
        })?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|err| {
            RuntimeError::ControlError(format!("symlink {}: {err}", link.display()).into())
        })?;
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).map_err(|err| {
            RuntimeError::ControlError(format!("symlink {}: {err}", link.display()).into())
        })?;
    }
    Ok(())
}

fn sanitize_relative_path(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    let mut clean = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(value) => clean.push(value),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_parent() {
        assert!(sanitize_relative_path("../bad.st").is_none());
        assert!(sanitize_relative_path("/abs/bad.st").is_none());
    }

    #[test]
    fn sanitize_accepts_nested() {
        let path = sanitize_relative_path("lib/util.st").unwrap();
        assert_eq!(path, PathBuf::from("lib/util.st"));
    }

    #[test]
    fn apply_deploy_writes_files() {
        let mut root = std::env::temp_dir();
        root.push(format!("trust-deploy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let request = DeployRequest {
            runtime_toml: Some("[runtime]\n".to_string()),
            io_toml: None,
            program_stbc_b64: Some(STANDARD.encode([1u8, 2, 3])),
            sources: Some(vec![DeploySource {
                path: "main.st".to_string(),
                content: "PROGRAM Main\nEND_PROGRAM\n".to_string(),
            }]),
            restart: None,
        };
        let result = apply_deploy(&root, request).unwrap();
        assert!(result.written.contains(&"runtime.toml".to_string()));
        assert!(root.join("program.stbc").exists());
        assert!(root.join("sources/main.st").exists());
        let _ = fs::remove_dir_all(root);
    }
}
