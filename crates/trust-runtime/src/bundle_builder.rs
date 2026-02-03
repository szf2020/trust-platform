//! Bundle build helpers (compile sources to program.stbc).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::harness::{CompileSession, SourceFile};

/// Build output summary for a bundle.
#[derive(Debug, Clone)]
pub struct BundleBuildReport {
    /// Written bytecode path (program.stbc).
    pub program_path: PathBuf,
    /// Source files included in the build.
    pub sources: Vec<PathBuf>,
}

/// Compile bundle sources into `program.stbc`.
pub fn build_program_stbc(
    bundle_root: &Path,
    sources_root: Option<&Path>,
) -> anyhow::Result<BundleBuildReport> {
    let default_sources = bundle_root.join("sources");
    let sources_root = sources_root.unwrap_or(&default_sources);
    if !sources_root.is_dir() {
        anyhow::bail!("sources directory not found: {}", sources_root.display());
    }

    let (sources, source_paths) = collect_sources(sources_root)?;
    if sources.is_empty() {
        anyhow::bail!(
            "no source files found in {} (expected .st/.pou files)",
            sources_root.display()
        );
    }

    let session = CompileSession::from_sources(sources);
    let bytes = session.build_bytecode_bytes()?;
    fs::create_dir_all(bundle_root)?;
    let program_path = bundle_root.join("program.stbc");
    fs::write(&program_path, bytes)?;

    Ok(BundleBuildReport {
        program_path,
        sources: source_paths,
    })
}

fn collect_sources(sources_root: &Path) -> anyhow::Result<(Vec<SourceFile>, Vec<PathBuf>)> {
    let patterns = ["**/*.st", "**/*.ST", "**/*.pou", "**/*.POU"];
    let mut seen = BTreeSet::new();
    let mut source_map = BTreeMap::new();

    for pattern in patterns {
        for entry in glob::glob(&format!("{}/{}", sources_root.display(), pattern))? {
            let path = entry?;
            if !path.is_file() {
                continue;
            }
            let resolved = path.canonicalize().unwrap_or_else(|_| path.clone());
            let path_text = resolved.to_string_lossy().to_string();
            if !seen.insert(path_text.clone()) {
                continue;
            }
            let text = fs::read_to_string(&resolved)?;
            source_map.insert(path_text, text);
        }
    }

    let mut sources = Vec::with_capacity(source_map.len());
    let mut paths = Vec::with_capacity(source_map.len());
    for (path, text) in source_map {
        paths.push(PathBuf::from(&path));
        sources.push(SourceFile::with_path(path, text));
    }
    Ok((sources, paths))
}
