//! Builds the Glean JSON fact set for the `obsidian.notes` schema.
//!
//! Entities that are referenced by other facts -- `File`, `Note`, `Tag` -- are
//! interned so each is emitted once and given a stable fact id; references to
//! them use `{ "id": N }`. Leaf facts are emitted without ids. Predicate
//! blocks are ordered so that every referenced fact is defined before it is
//! used, as required by Glean's JSON writer.

use crate::model::{Anchor, LinkKind, PropertyType, PropertyValue, Span};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const P_FILE: &str = "obsidian.notes.File.1";
const P_FILE_NAME: &str = "obsidian.notes.FileName.1";
const P_FILE_EXT: &str = "obsidian.notes.FileExtension.1";
const P_NOTE: &str = "obsidian.notes.Note.1";
const P_NOTE_TITLE: &str = "obsidian.notes.NoteTitle.1";
const P_NOTE_ALIAS: &str = "obsidian.notes.NoteAlias.1";
const P_NOTE_FRONTMATTER: &str = "obsidian.notes.NoteFrontmatter.1";
const P_TAG: &str = "obsidian.notes.Tag.1";
const P_TAG_PARENT: &str = "obsidian.notes.TagParent.1";
const P_NOTE_TAG: &str = "obsidian.notes.NoteTag.1";
const P_HAS_KEY: &str = "obsidian.notes.HasKey.1";
const P_HAS_KEY_TYPE: &str = "obsidian.notes.HasKeyType.1";
const P_HAS_KEY_VALUE: &str = "obsidian.notes.HasKeyValue.1";
const P_HEADING: &str = "obsidian.notes.Heading.1";
const P_BLOCK: &str = "obsidian.notes.Block.1";
const P_REFERENCE: &str = "obsidian.notes.Reference.1";
const P_UNRESOLVED: &str = "obsidian.notes.UnresolvedReference.1";
const P_LINK_ALIAS: &str = "obsidian.notes.LinkAlias.1";

/// Predicate block emission order: a referenced fact is always defined before
/// it is used (File < Note < NoteTag/Reference; Tag < TagParent/NoteTag).
const PREDICATE_ORDER: &[&str] = &[
    P_FILE,
    P_FILE_NAME,
    P_FILE_EXT,
    P_NOTE,
    P_NOTE_TITLE,
    P_NOTE_ALIAS,
    P_NOTE_FRONTMATTER,
    P_TAG,
    P_TAG_PARENT,
    P_NOTE_TAG,
    P_HAS_KEY,
    P_HAS_KEY_TYPE,
    P_HAS_KEY_VALUE,
    P_HEADING,
    P_BLOCK,
    P_REFERENCE,
    P_UNRESOLVED,
    P_LINK_ALIAS,
];

pub struct FactBuilder {
    next_id: u64,
    file_ids: HashMap<String, u64>,
    tag_ids: HashMap<String, u64>,
    link_aliases: HashSet<(String, String)>,
    facts: HashMap<&'static str, Vec<Value>>,
}

impl FactBuilder {
    pub fn new() -> Self {
        FactBuilder {
            next_id: 1,
            file_ids: HashMap::new(),
            tag_ids: HashMap::new(),
            link_aliases: HashSet::new(),
            facts: HashMap::new(),
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn push(&mut self, predicate: &'static str, fact: Value) {
        self.facts.entry(predicate).or_default().push(fact);
    }

    /// Intern a file by its vault-relative path, emitting `File`, `FileName`
    /// and (when present) `FileExtension` on first sight. Returns the File
    /// fact id.
    pub fn file(&mut self, path: &str, name: &str, extension: Option<&str>) -> u64 {
        if let Some(&id) = self.file_ids.get(path) {
            return id;
        }
        let id = self.alloc_id();
        self.file_ids.insert(path.to_string(), id);
        self.push(P_FILE, json!({ "id": id, "key": path }));
        self.push(
            P_FILE_NAME,
            json!({ "key": { "file": { "id": id }, "name": name } }),
        );
        if let Some(ext) = extension {
            self.push(
                P_FILE_EXT,
                json!({ "key": { "file": { "id": id }, "extension": ext } }),
            );
        }
        id
    }

    /// Look up an already-interned file id by path.
    pub fn file_id(&self, path: &str) -> Option<u64> {
        self.file_ids.get(path).copied()
    }

    /// Emit a `Note` fact (keyed by its File) and its `NoteTitle` (both the
    /// bare `title` and the vault-path `absolute` title). Returns the Note id.
    pub fn note(&mut self, file_id: u64, title: &str, absolute: &str) -> u64 {
        let id = self.alloc_id();
        self.push(P_NOTE, json!({ "id": id, "key": { "id": file_id } }));
        self.push(
            P_NOTE_TITLE,
            json!({ "key": { "note": { "id": id }, "title": title, "absolute": absolute } }),
        );
        id
    }

    /// Emit a frontmatter alias of a note.
    pub fn note_alias(&mut self, note_id: u64, alias: &str) {
        self.push(
            P_NOTE_ALIAS,
            json!({ "key": { "note": { "id": note_id }, "alias": alias } }),
        );
    }

    /// Record whether a note has a frontmatter block (emitted for every note).
    pub fn note_frontmatter(&mut self, note_id: u64, present: bool) {
        self.push(
            P_NOTE_FRONTMATTER,
            json!({ "key": { "note": { "id": note_id }, "present": present } }),
        );
    }

    /// Intern a tag by its (already normalized, lower-cased) name, emitting the
    /// `Tag` fact once. For a nested tag, each ancestor prefix is interned too
    /// and a `TagParent` links it to its immediate parent. Returns the tag id.
    pub fn tag(&mut self, name: &str) -> u64 {
        if let Some(&id) = self.tag_ids.get(name) {
            return id;
        }
        let id = self.alloc_id();
        self.tag_ids.insert(name.to_string(), id);
        self.push(P_TAG, json!({ "id": id, "key": name }));
        if let Some(pos) = name.rfind('/') {
            let parent_id = self.tag(&name[..pos]);
            self.push(
                P_TAG_PARENT,
                json!({ "key": { "tag": { "id": id }, "parent": { "id": parent_id } } }),
            );
        }
        id
    }

    /// Record an alias observed in a link to `target`, deduplicated globally.
    pub fn link_alias(&mut self, target: &str, alias: &str) {
        if !self
            .link_aliases
            .insert((target.to_string(), alias.to_string()))
        {
            return;
        }
        self.push(
            P_LINK_ALIAS,
            json!({ "key": { "target": target, "alias": alias } }),
        );
    }

    pub fn note_tag(&mut self, note_id: u64, tag_id: u64) {
        self.push(
            P_NOTE_TAG,
            json!({ "key": { "note": { "id": note_id }, "tag": { "id": tag_id } } }),
        );
    }

    pub fn has_key(&mut self, note_id: u64, key: &str) {
        self.push(
            P_HAS_KEY,
            json!({ "key": { "note": { "id": note_id }, "key": key } }),
        );
    }

    pub fn has_key_type(&mut self, note_id: u64, key: &str, ptype: PropertyType) {
        self.push(
            P_HAS_KEY_TYPE,
            json!({ "key": { "note": { "id": note_id }, "key": key, "type_": ptype.index() } }),
        );
    }

    pub fn has_key_value(&mut self, note_id: u64, key: &str, value: &PropertyValue) {
        self.push(
            P_HAS_KEY_VALUE,
            json!({ "key": { "note": { "id": note_id }, "key": key, "value": property_value_json(value) } }),
        );
    }

    pub fn heading(&mut self, note_id: u64, text: &str, level: u64, span: Span) {
        self.push(
            P_HEADING,
            json!({ "key": {
                "note": { "id": note_id },
                "text": text,
                "level": level,
                "span": span_json(span),
            }}),
        );
    }

    pub fn block(&mut self, note_id: u64, id: &str, span: Span) {
        self.push(
            P_BLOCK,
            json!({ "key": {
                "note": { "id": note_id },
                "id": id,
                "span": span_json(span),
            }}),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reference(
        &mut self,
        source_note_id: u64,
        target_file_id: u64,
        kind: LinkKind,
        span: Span,
        anchor: &Anchor,
        alias: Option<&str>,
    ) {
        let mut key = json!({
            "source": { "id": source_note_id },
            "target": { "id": target_file_id },
            "kind": kind.index(),
            "span": span_json(span),
            "anchor": anchor_json(anchor),
        });
        if let Some(a) = alias {
            key["alias"] = json!(a); // `maybe string`: present only when set
        }
        self.push(P_REFERENCE, json!({ "key": key }));
    }

    pub fn unresolved_reference(
        &mut self,
        source_note_id: u64,
        target: &str,
        kind: LinkKind,
        span: Span,
        alias: Option<&str>,
    ) {
        let mut key = json!({
            "source": { "id": source_note_id },
            "target": target,
            "kind": kind.index(),
            "span": span_json(span),
        });
        if let Some(a) = alias {
            key["alias"] = json!(a);
        }
        self.push(P_UNRESOLVED, json!({ "key": key }));
    }

    /// Assemble the final Glean JSON document: an array of predicate blocks in
    /// dependency order, omitting predicates that have no facts.
    pub fn finish(mut self) -> Value {
        let mut out = Vec::new();
        for &predicate in PREDICATE_ORDER {
            if let Some(facts) = self.facts.remove(predicate) {
                if !facts.is_empty() {
                    out.push(json!({ "predicate": predicate, "facts": facts }));
                }
            }
        }
        Value::Array(out)
    }
}

impl Default for FactBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn span_json(span: Span) -> Value {
    json!({ "start": span.start, "length": span.length })
}

fn anchor_json(anchor: &Anchor) -> Value {
    match anchor {
        Anchor::None => json!({ "none": {} }),
        Anchor::Heading(h) => json!({ "heading": h }),
        Anchor::Block(b) => json!({ "block": b }),
    }
}

fn property_value_json(value: &PropertyValue) -> Value {
    match value {
        PropertyValue::Text(s) => json!({ "text": s }),
        PropertyValue::Number(s) => json!({ "number": s }),
        PropertyValue::Checkbox(b) => json!({ "checkbox": b }),
        PropertyValue::Date(s) => json!({ "date": s }),
        PropertyValue::Datetime(s) => json!({ "datetime": s }),
    }
}
