//! End-to-end test: index `examples/obsidian-vault` with the Obsidian dialect
//! and assert on both the base `markdown.*` and the `obsidian.*` facts.

use markdown_glean::index_corpus;
use obsidian_glean::ObsidianDialect;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn facts() -> Value {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/obsidian-vault");
    index_corpus(&dir, ObsidianDialect::default())
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

fn file_paths(f: &Value) -> HashMap<u64, String> {
    f.as_array()
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
        .collect()
}

/// obsidian.Note id -> title.
fn note_titles(f: &Value) -> HashMap<u64, String> {
    keys(f, "obsidian.NoteTitle.1")
        .iter()
        .map(|k| {
            (
                k["note"]["id"].as_u64().unwrap(),
                k["title"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn note_by_title(f: &Value, title: &str) -> u64 {
    note_titles(f)
        .into_iter()
        .find(|(_, t)| t == title)
        .unwrap_or_else(|| panic!("no note {title}"))
        .0
}

#[test]
fn base_and_obsidian_facts_coexist() {
    let f = facts();
    assert_eq!(count(&f, "markdown.File.1"), 7, "6 notes + 1 attachment");
    assert_eq!(count(&f, "markdown.Document.1"), 6);
    assert_eq!(count(&f, "obsidian.Note.1"), 6);
    // Base facts are emitted for a vault too.
    assert!(count(&f, "markdown.Heading.1") >= 6);
    assert!(count(&f, "markdown.CodeBlock.1") >= 2);
}

#[test]
fn note_titles_and_aliases() {
    let f = facts();
    let alpha = note_by_title(&f, "Alpha");
    let nt = keys(&f, "obsidian.NoteTitle.1")
        .into_iter()
        .find(|k| k["note"]["id"].as_u64() == Some(alpha))
        .unwrap();
    assert_eq!(nt["absolute"].as_str(), Some("Projects/Alpha"));

    let welcome = note_by_title(&f, "Welcome");
    let aliases: HashSet<_> = keys(&f, "obsidian.NoteAlias.1")
        .iter()
        .filter(|k| k["note"]["id"].as_u64() == Some(welcome))
        .map(|k| k["alias"].as_str().unwrap().to_string())
        .collect();
    assert!(aliases.contains("Home") && aliases.contains("Start Here"));
}

#[test]
fn obsidian_property_types() {
    let f = facts();
    let welcome = note_by_title(&f, "Welcome");
    let mut types = HashMap::new();
    for k in keys(&f, "obsidian.NotePropertyType.1") {
        if k["note"]["id"].as_u64() == Some(welcome) {
            types.insert(
                k["key"].as_str().unwrap().to_string(),
                k["type_"].as_u64().unwrap(),
            );
        }
    }
    assert_eq!(types["tags"], 1); // list
    assert_eq!(types["priority"], 2); // number
    assert_eq!(types["publish"], 3); // checkbox
    assert_eq!(types["due"], 4); // date
    assert_eq!(types["reviewed"], 5); // datetime
}

#[test]
fn wiki_references_with_anchors_and_embeds() {
    let f = facts();
    let welcome = note_by_title(&f, "Welcome");
    let paths = file_paths(&f);
    let mut anchors = HashSet::new();
    let mut embed_targets = HashSet::new();
    for k in keys(&f, "obsidian.WikiReference.1") {
        if k["source"]["id"].as_u64() != Some(welcome) {
            continue;
        }
        let target = &paths[&k["target"]["id"].as_u64().unwrap()];
        if target == "Projects/Alpha.md" {
            anchors.insert(k["anchor"].clone());
        }
        if k["kind"].as_u64() == Some(1) {
            embed_targets.insert(target.clone());
        }
    }
    assert!(anchors.contains(&serde_json::json!({ "none_": {} })));
    assert!(anchors.contains(&serde_json::json!({ "heading": "Goals" })));
    assert!(anchors.contains(&serde_json::json!({ "block": "key-point" })));
    assert!(embed_targets.contains("attachments/diagram.png"));
}

#[test]
fn unresolved_wiki_and_link_aliases() {
    let f = facts();
    let unresolved: HashSet<_> = keys(&f, "obsidian.UnresolvedWikiReference.1")
        .iter()
        .map(|k| k["target"].as_str().unwrap().to_string())
        .collect();
    assert!(unresolved.contains("Nonexistent Note"));
    assert!(unresolved.contains("Does not exist"));

    let link_aliases: HashSet<(String, String)> = keys(&f, "obsidian.LinkAlias.1")
        .iter()
        .map(|k| {
            (
                k["target"].as_str().unwrap().to_string(),
                k["alias"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(link_aliases.contains(&("Beta".into(), "the beta project".into())));
    assert!(link_aliases.contains(&("Does not exist".into(), "missing".into())));
}

/// obsidian.Tag id -> name (the Tag key is a bare string).
fn tag_names(f: &Value) -> HashMap<u64, String> {
    f.as_array()
        .unwrap()
        .iter()
        .find(|b| b["predicate"].as_str().unwrap().ends_with("obsidian.Tag.1"))
        .map(|b| {
            b["facts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| {
                    (
                        x["id"].as_u64().unwrap(),
                        x["key"].as_str().unwrap().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn tags_lowercased_and_nested_with_parents() {
    let f = facts();
    let tag_names = tag_names(&f);
    let set: HashSet<_> = tag_names.values().cloned().collect();
    assert!(set.contains("intro") && !set.contains("Intro") && !set.contains("INTRO"));
    for t in ["2025", "2025/12", "2025/12/20"] {
        assert!(set.contains(t), "missing ancestor {t}");
    }
    let parents: HashSet<(String, String)> = keys(&f, "obsidian.TagParent.1")
        .iter()
        .map(|k| {
            (
                tag_names[&k["tag"]["id"].as_u64().unwrap()].clone(),
                tag_names[&k["parent"]["id"].as_u64().unwrap()].clone(),
            )
        })
        .collect();
    assert!(parents.contains(&("2025/12/20".into(), "2025/12".into())));
}

#[test]
fn blocks_and_task_attribution() {
    let f = facts();
    let alpha = note_by_title(&f, "Alpha");
    assert!(keys(&f, "obsidian.Block.1")
        .iter()
        .any(|k| k["note"]["id"].as_u64() == Some(alpha) && k["id"].as_str() == Some("key-point")));

    // The Features "Review [[Projects/Alpha]] and tag #meeting" task attributes
    // its tag (obsidian.TaskItemTag) and its wiki link (markdown.TaskItemLink).
    assert!(count(&f, "obsidian.TaskItemTag.1") >= 1);
    assert!(count(&f, "markdown.TaskItemLink.1") >= 1);
}

/// obsidian.Note id -> markdown.Document id.
fn note_to_doc(f: &Value) -> HashMap<u64, u64> {
    f.as_array()
        .unwrap()
        .iter()
        .find(|b| {
            b["predicate"]
                .as_str()
                .unwrap()
                .ends_with("obsidian.Note.1")
        })
        .unwrap()["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| (x["id"].as_u64().unwrap(), x["key"]["id"].as_u64().unwrap()))
        .collect()
}

#[test]
fn reference_style_link_resolves_in_vault() {
    let f = facts();
    let features = note_by_title(&f, "Features");
    let features_doc = note_to_doc(&f)[&features];
    let paths = file_paths(&f);
    // `[the Alpha note][alpha]` is a base markdown reference to Projects/Alpha.md.
    let resolved = keys(&f, "markdown.Reference.1").iter().any(|k| {
        k["source"]["id"].as_u64() == Some(features_doc)
            && paths[&k["target"]["id"].as_u64().unwrap()] == "Projects/Alpha.md"
    });
    assert!(resolved);
}
