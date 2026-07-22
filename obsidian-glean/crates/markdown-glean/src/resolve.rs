//! Link-target resolution.
//!
//! `Resolve` maps a link destination path to a corpus-relative file path. The
//! base [`PathResolver`] does standard relative-path resolution; a dialect can
//! provide a richer resolver (e.g. Obsidian's note-name + alias resolution) and
//! the base link emitter will use it for Markdown links too.

use std::collections::HashSet;

pub trait Resolve {
    /// Resolve a destination path (no `#fragment`) relative to `source_rel` to a
    /// corpus-relative file path, or `None` if it does not resolve.
    fn resolve(&self, target: &str, source_rel: &str) -> Option<String>;
}

/// Standard Markdown resolution: same-document (empty target), source-relative
/// path, then corpus-root path; each tried with and without a `.md` suffix.
pub struct PathResolver {
    pub paths: HashSet<String>,
}

impl PathResolver {
    pub fn new(paths: HashSet<String>) -> Self {
        PathResolver { paths }
    }
}

impl Resolve for PathResolver {
    fn resolve(&self, target: &str, source_rel: &str) -> Option<String> {
        let t = target.trim().trim_start_matches("./");
        if t.is_empty() {
            return Some(source_rel.to_string()); // same-document fragment link
        }
        if let Some(dir) = parent_dir(source_rel) {
            let joined = normalize_join(dir, t);
            if let Some(hit) = [joined.clone(), format!("{joined}.md")]
                .into_iter()
                .find(|c| self.paths.contains(c))
            {
                return Some(hit);
            }
        }
        [t.to_string(), format!("{t}.md")]
            .into_iter()
            .find(|c| self.paths.contains(c))
    }
}

pub fn parent_dir(rel: &str) -> Option<&str> {
    rel.rfind('/').map(|i| &rel[..i])
}

/// Join a directory and a relative target, resolving `.` and `..`.
pub fn normalize_join(dir: &str, rel: &str) -> String {
    let mut stack: Vec<&str> = if rel.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|c| !c.is_empty()).collect()
    };
    for comp in rel.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    stack.join("/")
}
