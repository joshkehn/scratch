//! YAML frontmatter handling, following Obsidian's conventions.
//!
//! Obsidian frontmatter is a YAML block fenced by `---` lines at the very top
//! of a file. Each mapping entry is a "property" with a name, an inferred type
//! (Text / List / Number / Checkbox / Date / Date & time) and one or more
//! values.

use crate::markdown::is_valid_tag;
use crate::model::{Property, PropertyType, PropertyValue};
use regex::Regex;
use std::sync::OnceLock;

/// Split leading YAML frontmatter from the body.
///
/// Returns `(Some(yaml), body, body_offset)` when the file opens with a `---`
/// line and has a matching closing `---` line, where `body_offset` is the byte
/// offset of `body` within `content`. Otherwise returns `(None, content, 0)`.
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

/// Parse a frontmatter YAML block into properties. A block that is not a YAML
/// mapping (or fails to parse) yields no properties.
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
        let (ptype, values) = classify(&v);
        props.push(Property { key, ptype, values });
    }
    props
}

/// The tags declared in frontmatter (the `tags` property), normalized without
/// a leading `#`. A list property contributes one tag per element; a scalar
/// string property is split on whitespace and commas (the legacy inline form).
pub fn frontmatter_tags(props: &[Property]) -> Vec<String> {
    let mut tags = Vec::new();
    for prop in props.iter().filter(|p| p.key == "tags") {
        match prop.ptype {
            PropertyType::List => {
                for v in &prop.values {
                    if let PropertyValue::Text(s) = v {
                        push_tag(&mut tags, s);
                    }
                }
            }
            _ => {
                for v in &prop.values {
                    if let PropertyValue::Text(s) = v {
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
    if is_valid_tag(name) {
        tags.push(name.to_string());
    }
}

/// Determine a property's type and values from its YAML value.
fn classify(v: &serde_yaml::Value) -> (PropertyType, Vec<PropertyValue>) {
    match v {
        serde_yaml::Value::Sequence(seq) => {
            let values = seq.iter().filter_map(scalar_value).collect();
            (PropertyType::List, values)
        }
        serde_yaml::Value::Null => (PropertyType::Text, Vec::new()),
        scalar => match scalar_value(scalar) {
            Some(pv) => {
                let ptype = match &pv {
                    PropertyValue::Number(_) => PropertyType::Number,
                    PropertyValue::Checkbox(_) => PropertyType::Checkbox,
                    PropertyValue::Date(_) => PropertyType::Date,
                    PropertyValue::Datetime(_) => PropertyType::Datetime,
                    PropertyValue::Text(_) => PropertyType::Text,
                };
                (ptype, vec![pv])
            }
            None => (PropertyType::Text, Vec::new()),
        },
    }
}

/// Convert a single YAML scalar into a typed property value. Returns `None`
/// for null.
fn scalar_value(v: &serde_yaml::Value) -> Option<PropertyValue> {
    match v {
        serde_yaml::Value::Bool(b) => Some(PropertyValue::Checkbox(*b)),
        serde_yaml::Value::Number(n) => Some(PropertyValue::Number(n.to_string())),
        serde_yaml::Value::String(s) => Some(classify_string(s)),
        serde_yaml::Value::Null => None,
        // Nested sequences/maps are not first-class Obsidian property types;
        // keep them as text so no information is silently dropped.
        other => Some(PropertyValue::Text(scalar_to_string(other))),
    }
}

/// Classify a string value as a Date, Date & time, or plain Text, mirroring
/// Obsidian's ISO-8601 date detection.
fn classify_string(s: &str) -> PropertyValue {
    if date_re().is_match(s) {
        PropertyValue::Date(s.to_string())
    } else if datetime_re().is_match(s) {
        PropertyValue::Datetime(s.to_string())
    } else {
        PropertyValue::Text(s.to_string())
    }
}

fn scalar_to_string(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Null => String::new(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .replace('\n', " "),
    }
}

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap())
}

fn datetime_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frontmatter_and_reports_body_offset() {
        let content = "---\ntitle: Hi\n---\n# Body\n";
        let (yaml, body, offset) = split_frontmatter(content);
        assert_eq!(yaml, Some("title: Hi\n"));
        assert_eq!(body, "# Body\n");
        assert_eq!(&content[offset..], "# Body\n");
    }

    #[test]
    fn no_frontmatter_when_missing_or_unterminated() {
        assert_eq!(split_frontmatter("# Just a note\n").0, None);
        assert_eq!(split_frontmatter("---\ntitle: x\nno close\n").0, None);
    }

    #[test]
    fn handles_crlf_fences() {
        let content = "---\r\nk: v\r\n---\r\nbody\r\n";
        let (yaml, body, _) = split_frontmatter(content);
        assert_eq!(yaml, Some("k: v\r\n"));
        assert_eq!(body, "body\r\n");
    }

    #[test]
    fn infers_all_property_types() {
        let props = parse_properties(
            "title: Hello\n\
             tags:\n  - a\n  - b\n\
             count: 42\n\
             ratio: 3.14\n\
             done: false\n\
             due: 2026-01-15\n\
             at: 2026-01-15T09:30:00\n",
        );
        let ty = |k: &str| props.iter().find(|p| p.key == k).unwrap().ptype;
        assert_eq!(ty("title"), PropertyType::Text);
        assert_eq!(ty("tags"), PropertyType::List);
        assert_eq!(ty("count"), PropertyType::Number);
        assert_eq!(ty("ratio"), PropertyType::Number);
        assert_eq!(ty("done"), PropertyType::Checkbox);
        assert_eq!(ty("due"), PropertyType::Date);
        assert_eq!(ty("at"), PropertyType::Datetime);
    }

    #[test]
    fn number_and_bool_values_are_faithful() {
        let props = parse_properties("count: 42\nratio: 3.14\ndone: true\n");
        let val = |k: &str| props.iter().find(|p| p.key == k).unwrap().values[0].clone();
        assert_eq!(val("count"), PropertyValue::Number("42".into()));
        assert_eq!(val("ratio"), PropertyValue::Number("3.14".into()));
        assert_eq!(val("done"), PropertyValue::Checkbox(true));
    }

    #[test]
    fn list_property_yields_one_value_per_element() {
        let props = parse_properties("aliases:\n  - one\n  - two\n");
        let p = &props[0];
        assert_eq!(p.ptype, PropertyType::List);
        assert_eq!(p.values.len(), 2);
    }

    #[test]
    fn frontmatter_tags_from_list_and_scalar() {
        let list = parse_properties("tags:\n  - a\n  - '#b'\n  - project/x\n");
        assert_eq!(frontmatter_tags(&list), vec!["a", "b", "project/x"]);
        let scalar = parse_properties("tags: a, b c\n");
        assert_eq!(frontmatter_tags(&scalar), vec!["a", "b", "c"]);
    }

    #[test]
    fn empty_property_has_no_values() {
        let props = parse_properties("empty:\n");
        assert_eq!(props[0].ptype, PropertyType::Text);
        assert!(props[0].values.is_empty());
    }
}
