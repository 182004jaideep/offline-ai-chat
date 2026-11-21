use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

pub fn models_dir(root: &Path) -> PathBuf {
    root.join("models")
}

/// Scans `<root>/models` for `.gguf` files, sorted by name for stable,
/// predictable ordering -- both for `--list-models` output and for picking
/// a default when no `--model` flag is given.
pub fn scan_models(root: &Path) -> Vec<ModelInfo> {
    let dir = models_dir(root);
    let mut models: Vec<ModelInfo> = match fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let is_gguf = path
                    .extension()
                    .map(|ext| ext.eq_ignore_ascii_case("gguf"))
                    .unwrap_or(false);
                if !is_gguf {
                    return None;
                }
                let name = path.file_stem()?.to_string_lossy().to_string();
                let size_bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
                Some(ModelInfo {
                    name,
                    path,
                    size_bytes,
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    models.sort_by(|a, b| a.name.cmp(&b.name));
    models
}

/// Resolves `--model` names (case-insensitive, matched against the file
/// stem) against the scanned registry. Unknown names come back as an
/// error rather than being silently dropped -- loading the wrong model (or
/// none) with no feedback is worse than a loud failure on an offline box.
pub fn resolve_requested<'a>(
    registry: &'a [ModelInfo],
    requested: &[String],
) -> Result<Vec<&'a ModelInfo>, Vec<String>> {
    let mut resolved = Vec::new();
    let mut unknown = Vec::new();
    for name in requested {
        match registry.iter().find(|m| m.name.eq_ignore_ascii_case(name)) {
            Some(m) => resolved.push(m),
            None => unknown.push(name.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(unknown);
    }
    Ok(resolved)
}

pub fn format_size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else {
        format!("{:.1} MB", b / MB)
    }
}
