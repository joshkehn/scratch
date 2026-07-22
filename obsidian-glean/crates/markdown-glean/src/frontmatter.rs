//! Generic frontmatter handling: split the leading `---` YAML block and type
//! each property against the generic value lattice (`FmType`/`FmValue`).
//!
//! A dialect (e.g. Obsidian) can narrow these generic types into its own
//! property types on top of the `Property` values produced here.

use crate::model::{FmType, FmValue, Property};
use regex::Regex;
use std::sync::OnceLock;

/// Split leading YAML frontmatter from the body.
///
/// Returns `(Some(yaml), body, body_offset)` when the file opens with a `---`
/// line and has a matching closing `---` line, where `body_offset` is the byte
/// offset of `body` within `content`. Otherwise `(None, content, 0)`.
pub fn split_frontmatter(content: &str) -> (Option<&str>, &str, usize) {
    let first_nl = content.find('\n');
    let first_line = match first_nl {
        Some(i) => &content[..i],
        None => content,
    };
    if first_line.trim_end_matches('\r') != "---" {
        return (None, content, 0);
    }
    let after_open = first_nl.map(|i| i + 1).unwrap_or(content.len());

    let mut pos = after_open;
    while pos < content.len() {
        let line_end = content[pos..]
            .find('\n')
            .map(|i| pos + i)
            .unwrap_or(content.len());
        let line = &content[pos..line_end];
        if line.trim_end_matches('\r') == "---" {
            let yaml = &content[after_open..pos];
            let body_start = (line_end + 1).min(content.len());
            return (Some(yaml), &content[body_start..], body_start);
        }
        pos = line_end + 1;
    }
    (None, content, 0)
}

/// Parse a frontmatter YAML block into generically-typed properties. A block
/// that is not a YAML mapping (or fails to parse) yields no properties.
pub fn parse_properties(yaml: &str) -> Vec<Property> {
    let value: serde_yaml::Value = match serde_yaml::from_str(yaml) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let map = match value {
        serde_yaml::Value::Mapping(m) => m,
        _ => return Vec::new(),
    };
    let mut props = Vec::new();
    for (k, v) in map {
        let key = match &k {
            serde_yaml::Value::String(s) => s.clone(),
            other => scalar_to_string(other),
        };
        let (ftype, values) = classify(&v);
        props.push(Property { key, ftype, values });
    }
    props
}

/// Determine a property's top-level type and its scalar value(s).
fn classify(v: &serde_yaml::Value) -> (FmType, Vec<FmValue>) {
    match v {
        serde_yaml::Value::Sequence(seq) => {
            let values = seq.iter().map(element_value).collect();
            (FmType::Array, values)
        }
        serde_yaml::Value::Mapping(_) => (FmType::Object, vec![FmValue::Object(to_json(v))]),
        serde_yaml::Value::Null => (FmType::Null, vec![FmValue::Null]),
        serde_yaml::Value::Bool(b) => (FmType::Boolean, vec![FmValue::Boolean(*b)]),
        serde_yaml::Value::Number(n) => (FmType::Number, vec![FmValue::Number(n.to_string())]),
        serde_yaml::Value::String(s) => {
            let val = classify_string(s);
            (fm_type_of(&val), vec![val])
        }
        other => {
            let val = FmValue::String(scalar_to_string(other));
            (FmType::String, vec![val])
        }
    }
}

/// Classify a single element of an array as a scalar value.
fn element_value(v: &serde_yaml::Value) -> FmValue {
    match v {
        serde_yaml::Value::Null => FmValue::Null,
        serde_yaml::Value::Bool(b) => FmValue::Boolean(*b),
        serde_yaml::Value::Number(n) => FmValue::Number(n.to_string()),
        serde_yaml::Value::String(s) => classify_string(s),
        // Nested arrays/objects inside a list are carried as JSON text.
        other => FmValue::Object(to_json(other)),
    }
}

fn fm_type_of(v: &FmValue) -> FmType {
    match v {
        FmValue::Null => FmType::Null,
        FmValue::Boolean(_) => FmType::Boolean,
        FmValue::Number(_) => FmType::Number,
        FmValue::String(_) => FmType::String,
        FmValue::Date(_) => FmType::Date,
        FmValue::Datetime(_) => FmType::Datetime,
        FmValue::Object(_) => FmType::Object,
    }
}

/// Classify a string as a Date, Date & time, or plain String (ISO-8601-ish),
/// mirroring how frontmatter dates are stored as strings by YAML parsers.
fn classify_string(s: &str) -> FmValue {
    if date_re().is_match(s) {
        FmValue::Date(s.to_string())
    } else if datetime_re().is_match(s) {
        FmValue::Datetime(s.to_string())
    } else {
        FmValue::String(s.to_string())
    }
}

fn scalar_to_string(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null => String::new(),
        other => to_json(other),
    }
}

/// Serialize a YAML value to compact JSON text.
fn to_json(v: &serde_yaml::Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap())
}

fn datetime_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2})?(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$")
            .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_and_reports_offset() {
        let content = "---\ntitle: Hi\n---\n# Body\n";
        let (yaml, body, offset) = split_frontmatter(content);
        assert_eq!(yaml, Some("title: Hi\n"));
        assert_eq!(body, "# Body\n");
        assert_eq!(&content[offset..], body);
    }

    #[test]
    fn no_frontmatter_when_missing_or_unterminated() {
        assert_eq!(split_frontmatter("# Note\n").0, None);
        assert_eq!(split_frontmatter("---\nk: v\nno close\n").0, None);
    }

    #[test]
    fn infers_generic_types() {
        let props = parse_properties(
            "s: hello\nn: 42\nf: 3.14\nb: true\nnull_v: null\nd: 2026-01-15\ndt: 2026-01-15T09:30:00Z\nlist:\n  - a\n  - b\nobj:\n  k: v\n",
        );
        let ty = |k: &str| props.iter().find(|p| p.key == k).unwrap().ftype;
        assert_eq!(ty("s"), FmType::String);
        assert_eq!(ty("n"), FmType::Number);
        assert_eq!(ty("f"), FmType::Number);
        assert_eq!(ty("b"), FmType::Boolean);
        assert_eq!(ty("null_v"), FmType::Null);
        assert_eq!(ty("d"), FmType::Date);
        assert_eq!(ty("dt"), FmType::Datetime);
        assert_eq!(ty("list"), FmType::Array);
        assert_eq!(ty("obj"), FmType::Object);
    }

    #[test]
    fn numbers_are_textual() {
        // Integers keep their form; unquoted floats are parsed (so `1.20`
        // normalizes to `1.2`). Quote a version-like value to keep the lexeme.
        let props = parse_properties("i: 42\nf: 1.20\nq: \"1.20\"\n");
        let val = |k: &str| props.iter().find(|p| p.key == k).unwrap().values[0].clone();
        assert_eq!(val("i"), FmValue::Number("42".into()));
        assert_eq!(val("f"), FmValue::Number("1.2".into()));
        assert_eq!(val("q"), FmValue::String("1.20".into()));
    }

    #[test]
    fn array_yields_one_value_per_element() {
        let props = parse_properties("tags:\n  - a\n  - b\n  - c\n");
        assert_eq!(props[0].ftype, FmType::Array);
        assert_eq!(props[0].values.len(), 3);
    }

    #[test]
    fn nested_object_is_json_text() {
        let props = parse_properties("meta:\n  a: 1\n  b: two\n");
        assert_eq!(props[0].ftype, FmType::Object);
        match &props[0].values[0] {
            FmValue::Object(json) => {
                assert!(json.contains("\"a\"") && json.contains("\"b\""));
            }
            other => panic!("expected object json, got {other:?}"),
        }
    }
}
