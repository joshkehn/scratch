//! Emission of `markdown.*` facts from the generic model.

use crate::facts::{fact_ref, FactBuilder, P_FILE, P_FILE_EXT, P_FILE_NAME};
use crate::model::{FmValue, Link, LinkKind, MarkdownContent, Property, Span};
use crate::resolve::Resolve;
use crate::scan::{is_external, percent_decode, url_scheme};
use serde_json::json;

pub const P_DOCUMENT: &str = "markdown.Document.1";
pub const P_FM_PRESENT: &str = "markdown.FrontmatterPresent.1";
pub const P_FM_KEY: &str = "markdown.FrontmatterKey.1";
pub const P_FM_TYPE: &str = "markdown.FrontmatterKeyType.1";
pub const P_FM_VALUE: &str = "markdown.FrontmatterKeyValue.1";
pub const P_HEADING: &str = "markdown.Heading.1";
pub const P_REFERENCE: &str = "markdown.Reference.1";
pub const P_EXTERNAL: &str = "markdown.ExternalLink.1";
pub const P_UNRESOLVED: &str = "markdown.UnresolvedReference.1";
pub const P_LINK_DEF: &str = "markdown.LinkRefDefinition.1";
pub const P_CODE_BLOCK: &str = "markdown.CodeBlock.1";
pub const P_HTML_ELEMENT: &str = "markdown.HtmlElement.1";
pub const P_HTML_ATTR: &str = "markdown.HtmlAttribute.1";
pub const P_TASK: &str = "markdown.TaskItem.1";
pub const P_TASK_LINK: &str = "markdown.TaskItemLink.1";
pub const P_TABLE: &str = "markdown.Table.1";
pub const P_TABLE_COLUMN: &str = "markdown.TableColumn.1";
pub const P_FOOTNOTE_DEF: &str = "markdown.FootnoteDefinition.1";
pub const P_FOOTNOTE_REF: &str = "markdown.FootnoteReference.1";

/// Base predicate emission order (referenced facts before their referrers).
pub const MARKDOWN_ORDER: &[&str] = &[
    P_FILE,
    P_FILE_NAME,
    P_FILE_EXT,
    P_DOCUMENT,
    P_FM_PRESENT,
    P_FM_KEY,
    P_FM_TYPE,
    P_FM_VALUE,
    P_HEADING,
    P_CODE_BLOCK,
    P_LINK_DEF,
    P_HTML_ELEMENT,
    P_HTML_ATTR,
    P_TASK,
    P_TASK_LINK,
    P_TABLE,
    P_TABLE_COLUMN,
    P_FOOTNOTE_DEF,
    P_FOOTNOTE_REF,
    P_REFERENCE,
    P_EXTERNAL,
    P_UNRESOLVED,
];

/// Result of emitting one document's base facts, for a dialect to build on.
pub struct DocEmit {
    /// (TaskItem fact id, whole-file line span) for each task item.
    pub task_ranges: Vec<(u64, Span)>,
}

/// Emit the `Document` fact (keyed by its File). Returns the Document fact id.
pub fn emit_document(sink: &mut FactBuilder, file_id: u64) -> u64 {
    sink.emit(P_DOCUMENT, fact_ref(file_id))
}

/// Emit frontmatter facts: presence, keys, types and values.
pub fn emit_frontmatter(sink: &mut FactBuilder, doc: u64, present: bool, props: &[Property]) {
    sink.emit_leaf(
        P_FM_PRESENT,
        json!({ "doc": fact_ref(doc), "present": present }),
    );
    for p in props {
        sink.emit_leaf(P_FM_KEY, json!({ "doc": fact_ref(doc), "key": p.key }));
        sink.emit_leaf(
            P_FM_TYPE,
            json!({ "doc": fact_ref(doc), "key": p.key, "type_": p.ftype.index() }),
        );
        for v in &p.values {
            sink.emit_leaf(
                P_FM_VALUE,
                json!({ "doc": fact_ref(doc), "key": p.key, "value": fm_value_json(v) }),
            );
        }
    }
}

/// Emit all body-content facts for a document, resolving links via `resolver`.
pub fn emit_content(
    sink: &mut FactBuilder,
    doc: u64,
    source_rel: &str,
    content: &MarkdownContent,
    resolver: &impl Resolve,
) -> DocEmit {
    for h in &content.headings {
        sink.emit_leaf(
            P_HEADING,
            json!({
                "doc": fact_ref(doc),
                "text": h.text,
                "level": h.level,
                "slug": h.slug,
                "kind": h.kind.index(),
                "span": span_json(h.span),
            }),
        );
    }
    for c in &content.code_blocks {
        sink.emit_leaf(
            P_CODE_BLOCK,
            json!({
                "doc": fact_ref(doc),
                "language": c.language,
                "info": c.info,
                "span": span_json(c.span),
            }),
        );
    }
    for d in &content.link_defs {
        sink.emit_leaf(
            P_LINK_DEF,
            json!({
                "doc": fact_ref(doc),
                "label": d.label,
                "destination": d.destination,
                "span": span_json(d.span),
            }),
        );
    }
    for el in &content.html {
        let eid = sink.emit(
            P_HTML_ELEMENT,
            json!({ "doc": fact_ref(doc), "name": el.name, "span": span_json(el.span) }),
        );
        for a in &el.attrs {
            sink.emit_leaf(
                P_HTML_ATTR,
                json!({ "element": fact_ref(eid), "name": a.name, "value": a.value }),
            );
        }
    }
    for t in &content.tables {
        let tid = sink.emit(
            P_TABLE,
            json!({
                "doc": fact_ref(doc),
                "columns": t.columns.len() as u64,
                "rows": t.rows,
                "span": span_json(t.span),
            }),
        );
        for (i, col) in t.columns.iter().enumerate() {
            sink.emit_leaf(
                P_TABLE_COLUMN,
                json!({
                    "table": fact_ref(tid),
                    "index": i as u64,
                    "align": col.align.index(),
                    "header": col.header,
                }),
            );
        }
    }
    for f in &content.footnote_defs {
        sink.emit_leaf(
            P_FOOTNOTE_DEF,
            json!({ "doc": fact_ref(doc), "label": f.label, "span": span_json(f.span) }),
        );
    }
    for f in &content.footnote_refs {
        sink.emit_leaf(
            P_FOOTNOTE_REF,
            json!({ "doc": fact_ref(doc), "label": f.label, "span": span_json(f.span) }),
        );
    }

    // Task items first, so links on a task line can be attributed to them.
    let task_ranges: Vec<(u64, Span)> = content
        .tasks
        .iter()
        .map(|t| {
            let id = sink.emit(
                P_TASK,
                json!({
                    "doc": fact_ref(doc),
                    "checked": t.checked,
                    "marker": t.marker,
                    "text": t.text,
                    "span": span_json(t.span),
                }),
            );
            (id, t.span)
        })
        .collect();

    for link in &content.links {
        emit_link(sink, doc, source_rel, link, resolver, &task_ranges);
    }

    DocEmit { task_ranges }
}

fn emit_link(
    sink: &mut FactBuilder,
    doc: u64,
    source_rel: &str,
    link: &Link,
    resolver: &impl Resolve,
    task_ranges: &[(u64, Span)],
) {
    // An autolink is always a URL/email, never an internal file reference.
    if is_external(&link.dest) || link.kind == LinkKind::Autolink {
        let mut scheme = url_scheme(&link.dest);
        if scheme.is_empty() && link.dest.contains('@') {
            scheme = "mailto".to_string(); // bare email autolink
        }
        sink.emit_leaf(
            P_EXTERNAL,
            json!({
                "source": fact_ref(doc),
                "url": link.dest,
                "scheme": scheme,
                "kind": link.kind.index(),
                "image": link.image,
                "text": link.text,
                "span": span_json(link.span),
            }),
        );
        return;
    }

    let (path, fragment) = split_fragment(&link.dest);
    let path = percent_decode(path);
    match resolver.resolve(&path, source_rel) {
        Some(target) => {
            let fid = sink.file_id(&target).expect("resolved file interned");
            let mut key = json!({
                "source": fact_ref(doc),
                "target": fact_ref(fid),
                "kind": link.kind.index(),
                "image": link.image,
                "text": link.text,
                "span": span_json(link.span),
            });
            if let Some(frag) = fragment {
                key["fragment"] = json!(frag);
            }
            sink.emit_leaf(P_REFERENCE, key);
            if let Some(task) = containing_task(link.span.start, task_ranges) {
                sink.emit_leaf(
                    P_TASK_LINK,
                    json!({ "task": fact_ref(task), "target": fact_ref(fid) }),
                );
            }
        }
        None => {
            sink.emit_leaf(
                P_UNRESOLVED,
                json!({
                    "source": fact_ref(doc),
                    "target": path,
                    "kind": link.kind.index(),
                    "image": link.image,
                    "text": link.text,
                    "span": span_json(link.span),
                }),
            );
        }
    }
}

/// Split a destination into (path, optional #fragment).
fn split_fragment(dest: &str) -> (&str, Option<String>) {
    match dest.split_once('#') {
        Some((p, f)) if !f.is_empty() => (p, Some(f.to_string())),
        _ => (dest, None),
    }
}

fn containing_task(pos: usize, tasks: &[(u64, Span)]) -> Option<u64> {
    tasks
        .iter()
        .find(|(_, s)| pos >= s.start && pos < s.end())
        .map(|(id, _)| *id)
}

fn span_json(span: Span) -> serde_json::Value {
    json!({ "start": span.start, "length": span.length })
}

fn fm_value_json(v: &FmValue) -> serde_json::Value {
    match v {
        FmValue::Null => json!({ "null_": {} }),
        FmValue::Boolean(b) => json!({ "boolean": b }),
        FmValue::Number(s) => json!({ "number": s }),
        FmValue::String(s) => json!({ "string_": s }),
        FmValue::Date(s) => json!({ "date": s }),
        FmValue::Datetime(s) => json!({ "datetime": s }),
        FmValue::Object(s) => json!({ "object": s }),
    }
}
