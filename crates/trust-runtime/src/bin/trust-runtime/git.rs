//! Git helper utilities for CLI workflows.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) fn git_init(root: &Path) -> anyhow::Result<()> {
    if root.join(".git").exists() {
        return Ok(());
    }
    if !git_available() {
        anyhow::bail!("git not found");
    }
    let status = Command::new("git").arg("init").current_dir(root).status()?;
    if !status.success() {
        anyhow::bail!("git init failed");
    }
    Ok(())
}

pub(crate) fn git_repo_root(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

pub(crate) fn git_output(root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
