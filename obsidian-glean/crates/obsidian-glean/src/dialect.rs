//! The Obsidian [`Dialect`] implementation: cross-vault name/alias resolution,
//! and emission of `obsidian.*` facts on top of the base `markdown.*` facts.

use crate::model::{Anchor, ObsidianContent, PropertyType, WikiLink};
use crate::scan::{self, normalize_tag};
use markdown_glean::corpus::DEFAULT_DOC_EXTENSIONS;
use markdown_glean::fact_ref;
use markdown_glean::model::{FmType, FmValue, Property, Span};
use markdown_glean::path::{basename, is_document, stem};
use markdown_glean::resolve::{normalize_join, parent_dir, Resolve};
use markdown_glean::{Corpus, Dialect, DocContext, DocEmit, FactBuilder, MarkdownContent};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

const P_NOTE: &str = "obsidian.Note.1";
const P_NOTE_TITLE: &str = "obsidian.NoteTitle.1";
const P_NOTE_ALIAS: &str = "obsidian.NoteAlias.1";
const P_TAG: &str = "obsidian.Tag.1";
const P_TAG_PARENT: &str = "obsidian.TagParent.1";
const P_NOTE_TAG: &str = "obsidian.NoteTag.1";
const P_TASK_TAG: &str = "obsidian.TaskItemTag.1";
const P_BLOCK: &str = "obsidian.Block.1";
const P_NOTE_PROP_TYPE: &str = "obsidian.NotePropertyType.1";
const P_WIKI_REF: &str = "obsidian.WikiReference.1";
const P_WIKI_UNRESOLVED: &str = "obsidian.UnresolvedWikiReference.1";
const P_LINK_ALIAS: &str = "obsidian.LinkAlias.1";
/// Reused base predicate: a resolved wiki reference inside a task item.
const P_TASK_LINK: &str = "markdown.TaskItemLink.1";

/// Obsidian predicate emission order (appended after the base order).
pub const OBSIDIAN_ORDER: &[&str] = &[
    P_NOTE,
    P_NOTE_TITLE,
    P_NOTE_ALIAS,
    P_TAG,
    P_TAG_PARENT,
    P_NOTE_TAG,
    P_TASK_TAG,
    P_BLOCK,
    P_NOTE_PROP_TYPE,
    P_WIKI_REF,
    P_WIKI_UNRESOLVED,
    P_LINK_ALIAS,
];

/// The Obsidian dialect. Holds the cross-vault resolution indices built in
/// `prepare`.
#[derive(Default)]
pub struct ObsidianDialect {
    paths: HashSet<String>,
    /// note name / basename -> file paths.
    name_index: HashMap<String, Vec<String>>,
    name_index_ci: HashMap<String, Vec<String>>,
    /// frontmatter alias -> file path.
    alias_index: HashMap<String, String>,
}

impl Resolve for ObsidianDialect {
    fn resolve(&self, target: &str, source_rel: &str) -> Option<String> {
        let t = target.trim().trim_start_matches("./");
        if t.is_empty() {
            return Some(source_rel.to_string());
        }
        if let Some(dir) = parent_dir(source_rel) {
            let joined = normalize_join(dir, t);
            for cand in [joined.clone(), format!("{joined}.md")] {
                if self.paths.contains(&cand) {
                    return Some(cand);
                }
            }
        }
        for cand in [t.to_string(), format!("{t}.md")] {
            if self.paths.contains(&cand) {
                return Some(cand);
            }
        }
        let base = t.rsplit('/').next().unwrap_or(t);
        if let Some(p) = pick(self.name_index.get(base)) {
            return Some(p);
        }
        if let Some(p) = pick(self.name_index_ci.get(&base.to_lowercase())) {
            return Some(p);
        }
        self.alias_index.get(t).cloned()
    }
}

impl Dialect for ObsidianDialect {
    type Content = ObsidianContent;

    fn predicate_order(&self) -> &'static [&'static str] {
        OBSIDIAN_ORDER
    }

    fn prepare(&mut self, corpus: &Corpus) {
        self.paths = corpus.path_set.clone();
        for rel in &corpus.files {
            let base = basename(rel);
            add_name(&mut self.name_index, &mut self.name_index_ci, base, rel);
            if is_document(rel, DEFAULT_DOC_EXTENSIONS) {
                let s = stem(rel, DEFAULT_DOC_EXTENSIONS);
                add_name(&mut self.name_index, &mut self.name_index_ci, s, rel);
            }
        }
        for doc in &corpus.docs {
            for alias in aliases(&doc.properties) {
                self.alias_index
                    .entry(alias)
                    .or_insert_with(|| doc.rel.clone());
            }
        }
    }

    fn scan(&self, ctx: &DocContext) -> (ObsidianContent, Vec<(usize, usize)>) {
        scan::scan(ctx.body, ctx.body_offset)
    }

    fn emit(
        &self,
        ctx: &DocContext,
        content: &ObsidianContent,
        _md: &MarkdownContent,
        base: &DocEmit,
        sink: &mut FactBuilder,
    ) {
        let note = sink.emit(P_NOTE, fact_ref(ctx.doc_id));

        // Title + absolute title.
        let title = stem(ctx.rel, DEFAULT_DOC_EXTENSIONS);
        let absolute = match parent_dir(ctx.rel) {
            Some(dir) => format!("{dir}/{title}"),
            None => title.to_string(),
        };
        sink.emit_leaf(
            P_NOTE_TITLE,
            json!({ "note": fact_ref(note), "title": title, "absolute": absolute }),
        );

        // Aliases and Obsidian property types.
        for alias in aliases(ctx.properties) {
            sink.emit_leaf(
                P_NOTE_ALIAS,
                json!({ "note": fact_ref(note), "alias": alias }),
            );
        }
        for prop in ctx.properties {
            sink.emit_leaf(
                P_NOTE_PROP_TYPE,
                json!({ "note": fact_ref(note), "key": prop.key, "type_": obsidian_type(prop.ftype).index() }),
            );
        }

        // Tags: frontmatter `tags` + inline `#tags`.
        let mut note_tags: HashSet<u64> = HashSet::new();
        for tag in frontmatter_tags(ctx.properties) {
            if let Some(name) = normalize_tag(&tag) {
                let tid = intern_tag(sink, &name);
                if note_tags.insert(tid) {
                    sink.emit_leaf(
                        P_NOTE_TAG,
                        json!({ "note": fact_ref(note), "tag": fact_ref(tid) }),
                    );
                }
            }
        }
        for occ in &content.tags {
            if let Some(name) = normalize_tag(&occ.name) {
                let tid = intern_tag(sink, &name);
                if note_tags.insert(tid) {
                    sink.emit_leaf(
                        P_NOTE_TAG,
                        json!({ "note": fact_ref(note), "tag": fact_ref(tid) }),
                    );
                }
                if let Some(task) = containing_task(occ.span.start, &base.task_ranges) {
                    sink.emit_leaf(
                        P_TASK_TAG,
                        json!({ "task": fact_ref(task), "tag": fact_ref(tid) }),
                    );
                }
            }
        }

        // Block ids.
        for block in &content.blocks {
            sink.emit_leaf(
                P_BLOCK,
                json!({ "note": fact_ref(note), "id": block.id, "span": span_json(block.span) }),
            );
        }

        // Wikilinks and embeds.
        for link in &content.wiki_links {
            self.emit_wiki(sink, note, ctx.rel, link, &base.task_ranges);
        }
    }
}

impl ObsidianDialect {
    fn emit_wiki(
        &self,
        sink: &mut FactBuilder,
        note: u64,
        source_rel: &str,
        link: &WikiLink,
        tasks: &[(u64, Span)],
    ) {
        let alias = link.alias.as_deref();
        match self.resolve(&link.target, source_rel) {
            Some(target) => {
                let fid = sink.file_id(&target).expect("resolved file interned");
                let mut key = json!({
                    "source": fact_ref(note),
                    "target": fact_ref(fid),
                    "kind": link.kind.index(),
                    "anchor": anchor_json(&link.anchor),
                    "span": span_json(link.span),
                });
                if let Some(a) = alias {
                    key["alias"] = json!(a);
                }
                sink.emit_leaf(P_WIKI_REF, key);
                if let Some(task) = containing_task(link.span.start, tasks) {
                    sink.emit_leaf(
                        P_TASK_LINK,
                        json!({ "task": fact_ref(task), "target": fact_ref(fid) }),
                    );
                }
            }
            None => {
                let mut key = json!({
                    "source": fact_ref(note),
                    "target": link.target,
                    "kind": link.kind.index(),
                    "anchor": anchor_json(&link.anchor),
                    "span": span_json(link.span),
                });
                if let Some(a) = alias {
                    key["alias"] = json!(a);
                }
                sink.emit_leaf(P_WIKI_UNRESOLVED, key);
            }
        }
        if let Some(a) = alias {
            if !link.target.is_empty() && a != link.target {
                sink.emit_leaf_once(
                    P_LINK_ALIAS,
                    &format!("{}\u{0}{a}", link.target),
                    || json!({ "target": link.target, "alias": a }),
                );
            }
        }
    }
}

/// Intern an obsidian `Tag` (and every ancestor prefix, with a `TagParent`
/// edge to its immediate parent). Returns the tag fact id.
fn intern_tag(sink: &mut FactBuilder, name: &str) -> u64 {
    if let Some(id) = sink.interned_id(P_TAG, name) {
        return id;
    }
    let id = sink.intern(P_TAG, name, || json!(name));
    if let Some(pos) = name.rfind('/') {
        let parent = intern_tag(sink, &name[..pos]);
        sink.emit_leaf(
            P_TAG_PARENT,
            json!({ "tag": fact_ref(id), "parent": fact_ref(parent) }),
        );
    }
    id
}

fn obsidian_type(ft: FmType) -> PropertyType {
    match ft {
        FmType::Array => PropertyType::List,
        FmType::Boolean => PropertyType::Checkbox,
        FmType::Number => PropertyType::Number,
        FmType::Date => PropertyType::Date,
        FmType::Datetime => PropertyType::Datetime,
        FmType::String | FmType::Null | FmType::Object => PropertyType::Text,
    }
}

fn frontmatter_tags(props: &[Property]) -> Vec<String> {
    let mut tags = Vec::new();
    for prop in props.iter().filter(|p| p.key == "tags" || p.key == "tag") {
        match prop.ftype {
            FmType::Array => {
                for v in &prop.values {
                    if let Some(s) = fm_str(v) {
                        push_tag(&mut tags, s);
                    }
                }
            }
            _ => {
                for v in &prop.values {
                    if let Some(s) = fm_str(v) {
                        for part in s.split([',', ' ', '\t']) {
                            push_tag(&mut tags, part);
                        }
                    }
                }
            }
        }
    }
    tags
}

fn push_tag(tags: &mut Vec<String>, raw: &str) {
    let name = raw.trim().trim_start_matches('#');
    if scan::is_valid_tag(name) {
        tags.push(name.to_string());
    }
}

fn aliases(props: &[Property]) -> Vec<String> {
    let mut out = Vec::new();
    for prop in props
        .iter()
        .filter(|p| p.key == "aliases" || p.key == "alias")
    {
        for v in &prop.values {
            if let Some(s) = fm_str(v) {
                let s = s.trim();
                if !s.is_empty() {
                    out.push(s.to_string());
                }
            }
        }
    }
    out
}

/// The string content of a scalar frontmatter value, if any.
fn fm_str(v: &FmValue) -> Option<&str> {
    match v {
        FmValue::String(s)
        | FmValue::Number(s)
        | FmValue::Date(s)
        | FmValue::Datetime(s)
        | FmValue::Object(s) => Some(s),
        FmValue::Null | FmValue::Boolean(_) => None,
    }
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

fn pick(candidates: Option<&Vec<String>>) -> Option<String> {
    candidates?
        .iter()
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
        .cloned()
}

fn containing_task(pos: usize, tasks: &[(u64, Span)]) -> Option<u64> {
    tasks
        .iter()
        .find(|(_, s)| pos >= s.start && pos < s.end())
        .map(|(id, _)| *id)
}

fn anchor_json(anchor: &Anchor) -> Value {
    match anchor {
        Anchor::None => json!({ "none_": {} }),
        Anchor::Heading(h) => json!({ "heading": h }),
        Anchor::Block(b) => json!({ "block": b }),
    }
}

fn span_json(span: Span) -> Value {
    json!({ "start": span.start, "length": span.length })
}
