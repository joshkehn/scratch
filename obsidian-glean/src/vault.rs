//! Walks an Obsidian vault on disk and drives fact emission.
//!
//! Processing happens in two phases:
//!   1. Collect every file, intern `File`/`Note` facts, and build the indices
//!      used to resolve links (by path, by note name, and by alias).
//!   2. For each note, emit frontmatter, tag, heading, block and (resolved)
//!      reference facts.

use crate::facts::FactBuilder;
use crate::frontmatter::{frontmatter_tags, parse_properties, split_frontmatter};
use crate::markdown;
use crate::model::{Property, PropertyValue};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use walkdir::WalkDir;

/// Summary counts from an indexing run, reported to stderr.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub files: usize,
    pub notes: usize,
    pub references: usize,
    pub unresolved: usize,
    pub tags: usize,
}

struct NoteMeta {
    rel: String,
    note_id: u64,
    properties: Vec<Property>,
    body_offset: usize,
}

/// Index a vault rooted at `root`, returning the Glean JSON fact document and
/// run statistics.
pub fn index_vault(root: &Path) -> std::io::Result<(Value, Stats)> {
    let mut md_files: Vec<(String, String)> = Vec::new();
    let mut other_files: Vec<String> = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = match entry.path().strip_prefix(root) {
            Ok(p) => normalize_rel(p),
            Err(_) => continue,
        };
        if rel.split('/').any(|c| c.starts_with('.')) {
            continue; // skip dotfiles / .obsidian / .git etc.
        }
        if has_md_extension(&rel) {
            match std::fs::read_to_string(entry.path()) {
                Ok(content) => md_files.push((rel, content)),
                Err(_) => other_files.push(rel), // non-UTF-8: treat as a plain file
            }
        } else {
            other_files.push(rel);
        }
    }

    md_files.sort_by(|a, b| a.0.cmp(&b.0));
    other_files.sort();

    let mut fb = FactBuilder::new();
    let mut path_set: HashSet<String> = HashSet::new();
    let mut name_index: HashMap<String, Vec<String>> = HashMap::new();
    let mut name_index_ci: HashMap<String, Vec<String>> = HashMap::new();

    for (rel, _) in &md_files {
        register_file(
            rel,
            &mut fb,
            &mut path_set,
            &mut name_index,
            &mut name_index_ci,
        );
    }
    for rel in &other_files {
        register_file(
            rel,
            &mut fb,
            &mut path_set,
            &mut name_index,
            &mut name_index_ci,
        );
    }

    // Intern notes and collect their metadata; build the alias index.
    let mut notes: Vec<NoteMeta> = Vec::with_capacity(md_files.len());
    let mut alias_index: HashMap<String, String> = HashMap::new();
    for (rel, content) in &md_files {
        let file_id = fb.file_id(rel).expect("file interned above");
        let (yaml, _body, body_offset) = split_frontmatter(content);
        let properties = yaml.map(parse_properties).unwrap_or_default();
        let title = md_stem(basename(rel)).unwrap_or_else(|| basename(rel));
        // The "absolute" title is the vault path without ".md".
        let absolute = match parent_dir(rel) {
            Some(dir) => format!("{dir}/{title}"),
            None => title.to_string(),
        };
        let note_id = fb.note(file_id, title, &absolute);
        fb.note_frontmatter(note_id, yaml.is_some());
        for alias in aliases(&properties) {
            fb.note_alias(note_id, &alias);
            alias_index.entry(alias).or_insert_with(|| rel.clone());
        }
        notes.push(NoteMeta {
            rel: rel.clone(),
            note_id,
            properties,
            body_offset,
        });
    }

    let mut stats = Stats {
        files: md_files.len() + other_files.len(),
        notes: md_files.len(),
        ..Stats::default()
    };

    // Phase 2: per-note facts.
    for (i, meta) in notes.iter().enumerate() {
        let content = &md_files[i].1;
        let note_id = meta.note_id;
        let mut note_tags: HashSet<u64> = HashSet::new();

        // Frontmatter properties.
        for prop in &meta.properties {
            fb.has_key(note_id, &prop.key);
            fb.has_key_type(note_id, &prop.key, prop.ptype);
            for value in &prop.values {
                fb.has_key_value(note_id, &prop.key, value);
            }
        }
        // Frontmatter tags.
        for tag in frontmatter_tags(&meta.properties) {
            if let Some(name) = normalize_tag(&tag) {
                let tid = fb.tag(&name);
                if note_tags.insert(tid) {
                    fb.note_tag(note_id, tid);
                    stats.tags += 1;
                }
            }
        }

        // Body constructs.
        let body = &content[meta.body_offset..];
        let scanned = markdown::scan(body, meta.body_offset);

        // Task items first, so links/tags on a task line can be attributed to
        // the task by span containment. `todo_ranges` is (todo id, start, end).
        let todo_ranges: Vec<(u64, usize, usize)> = scanned
            .todos
            .iter()
            .map(|t| {
                let id = fb.todo(note_id, t.checked, &t.text, t.span);
                (id, t.span.start, t.span.start + t.span.length)
            })
            .collect();
        for cb in &scanned.code_blocks {
            fb.code_block(note_id, &cb.language, cb.span);
        }
        for el in &scanned.html {
            let eid = fb.html_element(note_id, &el.name, el.span);
            for attr in &el.attrs {
                fb.html_attribute(eid, &attr.name, &attr.value);
            }
        }

        for h in &scanned.headings {
            fb.heading(note_id, &h.text, h.level, h.span);
        }
        for b in &scanned.blocks {
            fb.block(note_id, &b.id, b.span);
        }
        for tag in &scanned.tags {
            if let Some(name) = normalize_tag(&tag.name) {
                let tid = fb.tag(&name);
                if note_tags.insert(tid) {
                    fb.note_tag(note_id, tid);
                    stats.tags += 1;
                }
                if let Some(todo_id) = containing_todo(tag.span.start, &todo_ranges) {
                    fb.todo_tag(todo_id, tid);
                }
            }
        }
        for link in &scanned.links {
            let alias = link.alias.as_deref();
            match resolve(
                &link.target,
                &meta.rel,
                &path_set,
                &name_index,
                &name_index_ci,
                &alias_index,
            ) {
                Some(target_path) => {
                    let fid = fb.file_id(&target_path).expect("resolved path interned");
                    fb.reference(note_id, fid, link.kind, link.span, &link.anchor, alias);
                    stats.references += 1;
                    if let Some(todo_id) = containing_todo(link.span.start, &todo_ranges) {
                        fb.todo_link(todo_id, fid);
                    }
                }
                None => {
                    fb.unresolved_reference(note_id, &link.target, link.kind, link.span, alias);
                    stats.unresolved += 1;
                }
            }
            // An observed alias for the target text, resolved or not. Skip the
            // trivial case where the alias just repeats the target.
            if let Some(a) = alias {
                if !link.target.is_empty() && a != link.target {
                    fb.link_alias(&link.target, a);
                }
            }
        }
    }

    Ok((fb.finish(), stats))
}

/// Resolve a link target to a vault-relative path, if possible. Tries, in
/// order: same-file (empty target), source-relative path, vault-root path,
/// note-name (basename, then case-insensitive), and alias.
fn resolve(
    target: &str,
    source_rel: &str,
    path_set: &HashSet<String>,
    name_index: &HashMap<String, Vec<String>>,
    name_index_ci: &HashMap<String, Vec<String>>,
    alias_index: &HashMap<String, String>,
) -> Option<String> {
    let t = target.trim().trim_start_matches("./");
    if t.is_empty() {
        return Some(source_rel.to_string()); // same-note anchor link
    }

    if let Some(dir) = parent_dir(source_rel) {
        let joined = normalize_join(dir, t);
        for cand in [joined.clone(), format!("{joined}.md")] {
            if path_set.contains(&cand) {
                return Some(cand);
            }
        }
    }

    for cand in [t.to_string(), format!("{t}.md")] {
        if path_set.contains(&cand) {
            return Some(cand);
        }
    }

    let base = t.rsplit('/').next().unwrap_or(t);
    if let Some(p) = pick(name_index.get(base)) {
        return Some(p);
    }
    if let Some(p) = pick(name_index_ci.get(&base.to_lowercase())) {
        return Some(p);
    }

    alias_index.get(t).cloned()
}

/// Choose a single path from a set of candidates deterministically: the
/// shortest, breaking ties lexicographically.
fn pick(candidates: Option<&Vec<String>>) -> Option<String> {
    candidates?
        .iter()
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
        .cloned()
}

/// Intern a file's `File` facts and register it in the path and name indices.
fn register_file(
    rel: &str,
    fb: &mut FactBuilder,
    path_set: &mut HashSet<String>,
    name_index: &mut HashMap<String, Vec<String>>,
    name_index_ci: &mut HashMap<String, Vec<String>>,
) {
    path_set.insert(rel.to_string());
    let base = basename(rel);
    add_name(name_index, name_index_ci, base, rel);
    if let Some(stem) = md_stem(base) {
        add_name(name_index, name_index_ci, stem, rel);
    }
    fb.file(rel, base, extension(rel).as_deref());
}

fn add_name(
    exact: &mut HashMap<String, Vec<String>>,
    ci: &mut HashMap<String, Vec<String>>,
    name: &str,
    rel: &str,
) {
    exact
        .entry(name.to_string())
        .or_default()
        .push(rel.to_string());
    ci.entry(name.to_lowercase())
        .or_default()
        .push(rel.to_string());
}

/// The id of the task item whose line span contains `pos`, if any.
fn containing_todo(pos: usize, todos: &[(u64, usize, usize)]) -> Option<u64> {
    todos
        .iter()
        .find(|&&(_, start, end)| pos >= start && pos < end)
        .map(|&(id, _, _)| id)
}

/// Normalize a tag: lower-case it (Obsidian tags are case-insensitive) and
/// drop a trailing slash. Returns `None` if nothing meaningful remains.
fn normalize_tag(name: &str) -> Option<String> {
    let n = name.trim().trim_end_matches('/').to_lowercase();
    if n.is_empty() {
        None
    } else {
        Some(n)
    }
}

fn aliases(props: &[Property]) -> Vec<String> {
    let mut out = Vec::new();
    for prop in props
        .iter()
        .filter(|p| p.key == "aliases" || p.key == "alias")
    {
        for value in &prop.values {
            if let PropertyValue::Text(s) = value {
                let s = s.trim();
                if !s.is_empty() {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
}

fn normalize_rel(p: &Path) -> String {
    p.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

/// Join a directory and a relative target, resolving `.` and `..` components.
fn normalize_join(dir: &str, rel: &str) -> String {
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

fn parent_dir(rel: &str) -> Option<&str> {
    rel.rfind('/').map(|i| &rel[..i])
}

fn basename(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn has_md_extension(rel: &str) -> bool {
    extension(rel).as_deref() == Some("md")
}

/// The base name of a Markdown file without its `.md` suffix, or `None` if the
/// name does not end in `.md` (case-insensitive).
fn md_stem(base: &str) -> Option<&str> {
    let n = base.len();
    if n >= 3 && base[n - 3..].eq_ignore_ascii_case(".md") {
        Some(&base[..n - 3])
    } else {
        None
    }
}

/// The lower-cased extension without the dot, or `None` for names with no
/// extension (including dotfiles like `.gitignore`).
fn extension(rel: &str) -> Option<String> {
    let base = basename(rel);
    let dot = base.rfind('.')?;
    if dot == 0 || dot + 1 >= base.len() {
        return None;
    }
    Some(base[dot + 1..].to_lowercase())
}
