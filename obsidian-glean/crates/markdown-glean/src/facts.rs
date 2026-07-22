//! The shared Glean fact accumulator.
//!
//! `FactBuilder` owns the single fact-id space and the `File` interner. Both
//! the base Markdown emitters and any dialect extension write through the same
//! builder, so a dialect fact (e.g. an Obsidian `WikiReference`) that resolves
//! to a base `File` references the exact id the base allocated.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

pub const P_FILE: &str = "markdown.File.1";
pub const P_FILE_NAME: &str = "markdown.FileName.1";
pub const P_FILE_EXT: &str = "markdown.FileExtension.1";

pub struct FactBuilder {
    next_id: u64,
    file_ids: HashMap<String, u64>,
    interned: HashMap<(&'static str, String), u64>,
    leaf_dedup: HashSet<(&'static str, String)>,
    facts: HashMap<&'static str, Vec<Value>>,
}

impl FactBuilder {
    pub fn new() -> Self {
        FactBuilder {
            next_id: 1,
            file_ids: HashMap::new(),
            interned: HashMap::new(),
            leaf_dedup: HashSet::new(),
            facts: HashMap::new(),
        }
    }

    /// Allocate a fresh fact id.
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn push(&mut self, predicate: &'static str, fact: Value) {
        self.facts.entry(predicate).or_default().push(fact);
    }

    /// Emit a leaf fact (no id, not referenced by other facts).
    pub fn emit_leaf(&mut self, predicate: &'static str, key: Value) {
        self.push(predicate, json!({ "key": key }));
    }

    /// Emit a leaf fact once per `(predicate, dedup)`; repeat calls are no-ops.
    pub fn emit_leaf_once(
        &mut self,
        predicate: &'static str,
        dedup: &str,
        build: impl FnOnce() -> Value,
    ) {
        if self.leaf_dedup.insert((predicate, dedup.to_string())) {
            self.push(predicate, json!({ "key": build() }));
        }
    }

    /// The id of an already-interned entity, if present.
    pub fn interned_id(&self, predicate: &'static str, dedup: &str) -> Option<u64> {
        self.interned.get(&(predicate, dedup.to_string())).copied()
    }

    /// Emit a fact with a fresh id, returning it so later facts can reference it.
    pub fn emit(&mut self, predicate: &'static str, key: Value) -> u64 {
        let id = self.alloc_id();
        self.push(predicate, json!({ "id": id, "key": key }));
        id
    }

    /// Intern a keyed entity: emit it once (with an id) and return the same id
    /// for repeat calls with the same `dedup` key. Use for referenced entities
    /// like tags. `build` produces the fact key only on first sight.
    pub fn intern(
        &mut self,
        predicate: &'static str,
        dedup: &str,
        build: impl FnOnce() -> Value,
    ) -> u64 {
        if let Some(&id) = self.interned.get(&(predicate, dedup.to_string())) {
            return id;
        }
        let id = self.alloc_id();
        self.interned.insert((predicate, dedup.to_string()), id);
        self.push(predicate, json!({ "id": id, "key": build() }));
        id
    }

    /// Intern a `File` by corpus-relative path, emitting `File`/`FileName` and
    /// (when present) `FileExtension` on first sight. Returns the File fact id.
    pub fn intern_file(&mut self, path: &str, name: &str, extension: Option<&str>) -> u64 {
        if let Some(&id) = self.file_ids.get(path) {
            return id;
        }
        let id = self.alloc_id();
        self.file_ids.insert(path.to_string(), id);
        self.push(P_FILE, json!({ "id": id, "key": path }));
        self.push(
            P_FILE_NAME,
            json!({ "key": { "file": fact_ref(id), "name": name } }),
        );
        if let Some(ext) = extension {
            self.push(
                P_FILE_EXT,
                json!({ "key": { "file": fact_ref(id), "extension": ext } }),
            );
        }
        id
    }

    /// Look up an already-interned File id by path.
    pub fn file_id(&self, path: &str) -> Option<u64> {
        self.file_ids.get(path).copied()
    }

    /// Assemble the Glean JSON document: predicate blocks in the given order,
    /// omitting empty ones. `order` must list every referenced fact's predicate
    /// before the predicates that reference it.
    pub fn finish(mut self, order: &[&'static str]) -> Value {
        let mut out = Vec::new();
        for &predicate in order {
            if let Some(facts) = self.facts.remove(predicate) {
                if !facts.is_empty() {
                    out.push(json!({ "predicate": predicate, "facts": facts }));
                }
            }
        }
        // Any predicate not listed in `order` is appended (deterministically) so
        // facts are never silently dropped.
        let mut leftover: Vec<_> = self
            .facts
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .collect();
        leftover.sort_by_key(|(p, _)| *p);
        for (predicate, facts) in leftover {
            out.push(json!({ "predicate": predicate, "facts": facts }));
        }
        Value::Array(out)
    }
}

impl Default for FactBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a `{ "id": N }` reference to another fact.
pub fn fact_ref(id: u64) -> Value {
    json!({ "id": id })
}
