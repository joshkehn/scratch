//! End-to-end test: index `examples/markdown-docs` with the base (no-dialect)
//! indexer and assert on the emitted `markdown.*` facts.

use markdown_glean::{index_corpus, NoDialect};
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

fn facts() -> Value {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/markdown-docs");
    index_corpus(&dir, NoDialect::default())
        .expect("indexing")
        .0
}

fn keys<'a>(f: &'a Value, suffix: &str) -> Vec<&'a Value> {
    f.as_array()
        .unwrap()
        .iter()
        .find(|b| b["predicate"].as_str().unwrap().ends_with(suffix))
        .map(|b| {
            b["facts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| &x["key"])
                .collect()
        })
        .unwrap_or_default()
}

fn count(f: &Value, suffix: &str) -> usize {
    keys(f, suffix).len()
}

#[test]
fn file_and_document_counts() {
    let f = facts();
    assert_eq!(count(&f, "markdown.File.1"), 3);
    assert_eq!(count(&f, "markdown.Document.1"), 3);
}

#[test]
fn atx_and_setext_headings_present() {
    let f = facts();
    let mut atx = false;
    let mut setext = false;
    for k in keys(&f, "markdown.Heading.1") {
        if k["text"] == "Overview" && k["kind"] == 1 {
            setext = true;
        }
        if k["text"] == "Installation" && k["kind"] == 0 {
            atx = true;
        }
    }
    assert!(setext, "the Setext 'Overview' heading");
    assert!(atx, "the ATX 'Installation' heading");
}

#[test]
fn frontmatter_typed_generically() {
    let f = facts();
    let mut types = std::collections::HashMap::new();
    for k in keys(&f, "markdown.FrontmatterKeyType.1") {
        types.insert(
            k["key"].as_str().unwrap().to_string(),
            k["type_"].as_u64().unwrap(),
        );
    }
    assert_eq!(types["title"], 3); // string
    assert_eq!(types["weight"], 2); // number
    assert_eq!(types["draft"], 1); // boolean
}

#[test]
fn table_with_columns() {
    let f = facts();
    let tables = keys(&f, "markdown.Table.1");
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0]["columns"], 3);
    assert_eq!(count(&f, "markdown.TableColumn.1"), 3);
}

#[test]
fn footnotes_present() {
    let f = facts();
    assert_eq!(count(&f, "markdown.FootnoteDefinition.1"), 1);
    assert_eq!(count(&f, "markdown.FootnoteReference.1"), 1);
}

#[test]
fn external_links_and_schemes() {
    let f = facts();
    let schemes: HashSet<_> = keys(&f, "markdown.ExternalLink.1")
        .iter()
        .map(|k| k["scheme"].as_str().unwrap().to_string())
        .collect();
    assert!(schemes.contains("https"));
    assert!(count(&f, "markdown.ExternalLink.1") >= 3);
}

#[test]
fn internal_references_resolve() {
    let f = facts();
    // The docs cross-link each other; find file paths and check a known edge.
    let file_paths: std::collections::HashMap<u64, String> = f
        .as_array()
        .unwrap()
        .iter()
        .find(|b| {
            b["predicate"]
                .as_str()
                .unwrap()
                .ends_with("markdown.File.1")
        })
        .unwrap()["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["id"].as_u64().unwrap(),
                x["key"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let targets: HashSet<_> = keys(&f, "markdown.Reference.1")
        .iter()
        .map(|k| file_paths[&k["target"]["id"].as_u64().unwrap()].clone())
        .collect();
    assert!(targets.contains("CONTRIBUTING.md"));
    assert!(targets.contains("docs/guide.md"));
}

#[test]
fn task_items_and_task_link() {
    let f = facts();
    // README roadmap + CONTRIBUTING checklist tasks.
    assert!(count(&f, "markdown.TaskItem.1") >= 5);
    // The "Render output, see [the guide]" task links docs/guide.md.
    assert!(count(&f, "markdown.TaskItemLink.1") >= 1);
}

#[test]
fn html_elements_and_attributes() {
    let f = facts();
    let names: HashSet<_> = keys(&f, "markdown.HtmlElement.1")
        .iter()
        .map(|k| k["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains("img") || names.contains("span"));
    assert!(count(&f, "markdown.HtmlAttribute.1") >= 1);
}
