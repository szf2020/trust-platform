//! Library dependency graph helpers.

use crate::config::{LibraryDependency, LibrarySpec, ProjectConfig};
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LibraryNode {
    pub name: String,
    pub version: Option<String>,
    pub path: PathBuf,
    pub dependencies: Vec<LibraryDependency>,
}

#[derive(Debug, Clone)]
pub struct LibraryGraph {
    pub nodes: Vec<LibraryNode>,
}

#[derive(Debug, Clone)]
pub struct LibraryIssue {
    pub code: &'static str,
    pub message: String,
    pub subject: String,
    pub dependency: Option<String>,
}

pub fn build_library_graph(config: &ProjectConfig) -> LibraryGraph {
    let nodes = config
        .libraries
        .iter()
        .map(|lib| LibraryNode {
            name: lib.name.clone(),
            version: lib.version.clone(),
            path: lib.path.clone(),
            dependencies: lib.dependencies.clone(),
        })
        .collect();
    LibraryGraph { nodes }
}

pub fn library_dependency_issues(config: &ProjectConfig) -> Vec<LibraryIssue> {
    let mut by_name: FxHashMap<&str, Vec<&LibrarySpec>> = FxHashMap::default();
    for lib in &config.libraries {
        by_name.entry(lib.name.as_str()).or_default().push(lib);
    }

    let mut issues = Vec::new();

    for (name, libs) in &by_name {
        let mut versions = BTreeSet::new();
        for lib in libs {
            versions.insert(version_label(&lib.version));
        }
        if versions.len() > 1 {
            let versions: Vec<String> = versions.into_iter().collect();
            issues.push(LibraryIssue {
                code: "L003",
                message: format!(
                    "Library '{name}' declared with conflicting versions ({})",
                    versions.join(", ")
                ),
                subject: (*name).to_string(),
                dependency: None,
            });
        }
    }

    for lib in &config.libraries {
        for dep in &lib.dependencies {
            let Some(candidates) = by_name.get(dep.name.as_str()) else {
                issues.push(LibraryIssue {
                    code: "L001",
                    message: format!(
                        "Library '{}' depends on '{}', but it is not configured",
                        lib.name, dep.name
                    ),
                    subject: lib.name.clone(),
                    dependency: Some(dep.name.clone()),
                });
                continue;
            };
            if let Some(required) = dep.version.as_deref() {
                let matched = candidates
                    .iter()
                    .any(|candidate| candidate.version.as_deref() == Some(required));
                if !matched {
                    let mut available: Vec<String> = candidates
                        .iter()
                        .map(|candidate| version_label(&candidate.version))
                        .collect();
                    available.sort();
                    available.dedup();
                    issues.push(LibraryIssue {
                        code: "L002",
                        message: format!(
                            "Library '{}' requires '{}' version {}, but available versions are {}",
                            lib.name,
                            dep.name,
                            required,
                            available.join(", ")
                        ),
                        subject: lib.name.clone(),
                        dependency: Some(dep.name.clone()),
                    });
                }
            }
        }
    }

    issues
}

fn version_label(version: &Option<String>) -> String {
    version
        .as_deref()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unspecified".to_string())
}
