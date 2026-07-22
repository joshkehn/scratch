//! Corpus-relative path helpers.

use std::path::Path;

/// Convert an OS path to a corpus-relative string with forward slashes.
pub fn normalize_rel(p: &Path) -> String {
    p.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// The base name (last path component).
pub fn basename(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// The lower-cased extension without the dot, or `None` (including dotfiles
/// like `.gitignore`).
pub fn extension(rel: &str) -> Option<String> {
    let base = basename(rel);
    let dot = base.rfind('.')?;
    if dot == 0 || dot + 1 >= base.len() {
        return None;
    }
    Some(base[dot + 1..].to_lowercase())
}

/// True if `rel` has one of the given (lower-cased) document extensions.
pub fn is_document(rel: &str, exts: &[&str]) -> bool {
    match extension(rel) {
        Some(e) => exts.contains(&e.as_str()),
        None => false,
    }
}

/// The base name without a trailing extension from `exts` (case-insensitive),
/// e.g. `stem("A/B/Note.md", &["md"]) == "Note"`.
pub fn stem<'a>(rel: &'a str, exts: &[&str]) -> &'a str {
    let base = basename(rel);
    if let Some(dot) = base.rfind('.') {
        if dot > 0 {
            let ext = base[dot + 1..].to_lowercase();
            if exts.contains(&ext.as_str()) {
                return &base[..dot];
            }
        }
    }
    base
}
