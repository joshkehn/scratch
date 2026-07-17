//! End-to-end test: index `examples/sample-vault` and assert on the emitted
//! Glean facts.

use obsidian_glean::index_vault;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn sample_facts() -> Value {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/sample-vault");
    index_vault(&dir).expect("indexing the sample vault").0
}

/// All fact `key` values for the predicate whose name ends with `suffix`.
fn keys<'a>(facts: &'a Value, suffix: &str) -> Vec<&'a Value> {
    facts
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["predicate"].as_str().unwrap().ends_with(suffix))
        .map(|b| {
            b["facts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| &f["key"])
                .collect()
        })
        .unwrap_or_default()
}

/// Number of facts for a predicate.
fn count(facts: &Value, suffix: &str) -> usize {
    keys(facts, suffix).len()
}

/// note fact id -> title
fn note_titles(facts: &Value) -> HashMap<u64, String> {
    keys(facts, ".NoteTitle.1")
        .iter()
        .map(|k| {
            (
                k["note"]["id"].as_u64().unwrap(),
                k["title"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// file fact id -> path
fn file_paths(facts: &Value) -> HashMap<u64, String> {
    facts
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["predicate"].as_str().unwrap().ends_with(".File.1"))
        .unwrap()["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["id"].as_u64().unwrap(),
                f["key"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn note_id_by_title(facts: &Value, title: &str) -> u64 {
    note_titles(facts)
        .into_iter()
        .find(|(_, t)| t == title)
        .unwrap_or_else(|| panic!("no note titled {title}"))
        .0
}

#[test]
fn file_and_note_counts() {
    let f = sample_facts();
    assert_eq!(count(&f, ".File.1"), 5, "4 notes + 1 attachment");
    assert_eq!(count(&f, ".Note.1"), 4);
    // The attachment has a name and an extension fact.
    let exts: HashSet<_> = keys(&f, ".FileExtension.1")
        .iter()
        .map(|k| k["extension"].as_str().unwrap().to_string())
        .collect();
    assert!(exts.contains("md"));
    assert!(exts.contains("png"));
}

#[test]
fn welcome_property_types_are_inferred() {
    let f = sample_facts();
    let welcome = note_id_by_title(&f, "Welcome");
    let mut types: HashMap<String, u64> = HashMap::new();
    for k in keys(&f, ".HasKeyType.1") {
        if k["note"]["id"].as_u64() == Some(welcome) {
            types.insert(
                k["key"].as_str().unwrap().to_string(),
                k["type_"].as_u64().unwrap(),
            );
        }
    }
    assert_eq!(types["title"], 0); // text
    assert_eq!(types["tags"], 1); // list
    assert_eq!(types["priority"], 2); // number
    assert_eq!(types["publish"], 3); // checkbox
    assert_eq!(types["due"], 4); // date
    assert_eq!(types["reviewed"], 5); // datetime
}

#[test]
fn resolved_references_carry_anchors_and_kinds() {
    let f = sample_facts();
    let welcome = note_id_by_title(&f, "Welcome");
    let paths = file_paths(&f);

    let mut anchors_to_alpha = HashSet::new();
    let mut embed_targets = HashSet::new();
    for k in keys(&f, ".Reference.1") {
        if k["source"]["id"].as_u64() != Some(welcome) {
            continue;
        }
        let target = &paths[&k["target"]["id"].as_u64().unwrap()];
        if target == "Projects/Alpha.md" {
            anchors_to_alpha.insert(k["anchor"].clone());
        }
        if k["kind"].as_u64() == Some(1) {
            embed_targets.insert(target.clone());
        }
    }
    assert!(anchors_to_alpha.contains(&serde_json::json!({ "none": {} })));
    assert!(anchors_to_alpha.contains(&serde_json::json!({ "heading": "Goals" })));
    assert!(anchors_to_alpha.contains(&serde_json::json!({ "block": "key-point" })));
    // The image embed resolved to the attachment.
    assert!(embed_targets.contains("attachments/diagram.png"));
}

#[test]
fn backlinks_are_bidirectional() {
    let f = sample_facts();
    let titles = note_titles(&f);
    let paths = file_paths(&f);
    let mut edges = HashSet::new();
    for k in keys(&f, ".Reference.1") {
        let src = titles[&k["source"]["id"].as_u64().unwrap()].clone();
        let tgt = paths[&k["target"]["id"].as_u64().unwrap()].clone();
        edges.insert((src, tgt));
    }
    assert!(edges.contains(&("Welcome".into(), "Projects/Alpha.md".into())));
    assert!(edges.contains(&("Alpha".into(), "Welcome.md".into())));
}

#[test]
fn dangling_link_is_unresolved() {
    let f = sample_facts();
    let unresolved: HashSet<_> = keys(&f, ".UnresolvedReference.1")
        .iter()
        .map(|k| k["target"].as_str().unwrap().to_string())
        .collect();
    assert!(unresolved.contains("Nonexistent Note"));
    assert_eq!(unresolved.len(), 1);
}

#[test]
fn tags_from_inline_and_frontmatter() {
    let f = sample_facts();
    let tags: HashSet<_> = keys(&f, ".Tag.1")
        .iter()
        .map(|k| k.as_str().unwrap().to_string())
        .collect();
    for expected in [
        "intro",
        "project/active",
        "area/work",
        "planning",
        "meeting",
    ] {
        assert!(tags.contains(expected), "missing tag {expected}");
    }
}

#[test]
fn code_masked_links_and_tags_are_absent() {
    let f = sample_facts();
    // No File was created for a fake link target, and no fake tags exist.
    let all = serde_json::to_string(&f).unwrap();
    for forbidden in [
        "NotALink",
        "AlsoIgnored",
        "nottag",
        "alsoignored",
        "stillignored",
    ] {
        assert!(
            !all.contains(forbidden),
            "code-fenced token leaked: {forbidden}"
        );
    }
}

#[test]
fn headings_and_blocks_present() {
    let f = sample_facts();
    let alpha = note_id_by_title(&f, "Alpha");
    let goals = keys(&f, ".Heading.1").iter().any(|k| {
        k["note"]["id"].as_u64() == Some(alpha)
            && k["text"].as_str() == Some("Goals")
            && k["level"].as_u64() == Some(2)
    });
    assert!(goals, "Alpha should have a level-2 'Goals' heading");
    let block = keys(&f, ".Block.1")
        .iter()
        .any(|k| k["note"]["id"].as_u64() == Some(alpha) && k["id"].as_str() == Some("key-point"));
    assert!(block, "Alpha should define block 'key-point'");
}
