//! Obsidian syntax extraction: `[[wikilinks]]`, `![[embeds]]`, `#tags` and
//! trailing `^block-id`s. Reuses the base crate's code-mask and line helpers so
//! that constructs inside code are ignored consistently with the base scanner.

use crate::model::{Anchor, BlockId, ObsidianContent, TagOccurrence, WikiKind, WikiLink};
use markdown_glean::model::Span;
use markdown_glean::scan::{compute_masks, in_ranges, lines};
use regex::Regex;
use std::sync::OnceLock;

fn wikilink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(!?)\[\[([^\[\]\n]+)\]\]").unwrap())
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Preceded by start-of-text, whitespace, or `>` (callout). `(` is excluded
    // so a Markdown link fragment `](#frag)` is not read as a tag.
    RE.get_or_init(|| Regex::new(r"(?:^|[\s>])(#[\p{L}\p{N}_/-]+)").unwrap())
}

/// True if a line is an ATX heading (`#`..`######` then a space), so a trailing
/// `^id` on it is part of the heading, not a block definition.
fn is_atx_heading(text: &str) -> bool {
    let t = text.trim_start();
    let hashes = t.bytes().take_while(|&b| b == b'#').count();
    (1..=6).contains(&hashes) && matches!(t.as_bytes().get(hashes), Some(b' ') | Some(b'\t'))
}

fn block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s)(\^[A-Za-z0-9][A-Za-z0-9-]*)\s*$").unwrap())
}

/// Scan a note body. Returns the Obsidian constructs (spans in whole-file
/// coordinates, i.e. `base` added) and the byte ranges the dialect reserves
/// (in body coordinates) so the generic scanner won't re-parse `[[...]]`.
pub fn scan(body: &str, base: usize) -> (ObsidianContent, Vec<(usize, usize)>) {
    let (masks, _fenced) = compute_masks(body);

    let mut wiki_links = Vec::new();
    let mut reserved: Vec<(usize, usize)> = Vec::new();
    for caps in wikilink_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masks.contains(m.start()) {
            continue;
        }
        let kind = if &caps[1] == "!" {
            WikiKind::Embed
        } else {
            WikiKind::Wikilink
        };
        let (target, anchor, parsed_alias) = parse_destination(&caps[2]);
        // For an embed, `|...` is a display size, not an alias.
        let alias = if kind == WikiKind::Wikilink {
            parsed_alias
        } else {
            None
        };
        wiki_links.push(WikiLink {
            kind,
            target,
            anchor,
            alias,
            span: Span::new(base + m.start(), m.len()),
        });
        reserved.push((m.start(), m.end()));
    }

    let mut tags = Vec::new();
    for caps in tag_re().captures_iter(body) {
        let g = caps.get(1).unwrap();
        if masks.contains(g.start()) || in_ranges(g.start(), &reserved) {
            continue;
        }
        let name = g.as_str()[1..].trim_end_matches('/');
        if !is_valid_tag(name) {
            continue;
        }
        tags.push(TagOccurrence {
            name: name.to_string(),
            span: Span::new(base + g.start(), g.len()),
        });
    }

    let mut blocks = Vec::new();
    for line in lines(body) {
        if masks.contains(line.start) || is_atx_heading(line.text) {
            continue;
        }
        if let Some(caps) = block_re().captures(line.text) {
            let g = caps.get(1).unwrap();
            blocks.push(BlockId {
                id: g.as_str()[1..].to_string(),
                span: Span::new(base + line.start + g.start(), g.len()),
            });
        }
    }

    (
        ObsidianContent {
            wiki_links,
            tags,
            blocks,
        },
        reserved,
    )
}

/// Split a wikilink inner into (target, anchor, alias). Handles table-escaped
/// pipes (`\|`) and `#heading` / `#^block` anchors.
fn parse_destination(raw: &str) -> (String, Anchor, Option<String>) {
    let (before_alias, alias) = match raw.split_once('|') {
        Some((left, right)) => {
            let a = right.trim().replace("\\|", "|");
            let alias = (!a.is_empty()).then_some(a);
            (left.trim_end_matches('\\'), alias)
        }
        None => (raw, None),
    };
    let (path, frag) = match before_alias.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (before_alias, None),
    };
    let anchor = match frag {
        None | Some("") => Anchor::None,
        Some(f) => {
            if let Some(block) = f.strip_prefix('^') {
                Anchor::Block(block.trim().to_string())
            } else {
                Anchor::Heading(f.trim().to_string())
            }
        }
    };
    (path.trim().to_string(), anchor, alias)
}

/// A tag must contain at least one non-numeric character (so `#1984` is not a
/// tag but `#y1984` is), and must be non-empty.
pub fn is_valid_tag(name: &str) -> bool {
    !name.is_empty() && name.chars().any(|c| !c.is_ascii_digit())
}

/// Normalize a tag: lower-case it and drop a trailing slash. `None` if empty.
pub fn normalize_tag(name: &str) -> Option<String> {
    let n = name.trim().trim_end_matches('/').to_lowercase();
    if n.is_empty() {
        None
    } else {
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikilinks_embeds_anchors_aliases() {
        let (c, reserved) = scan(
            "[[Alpha]] ![[img.png]] [[Note#Heading|shown]] [[N#^blk]]",
            0,
        );
        assert_eq!(c.wiki_links[0].kind, WikiKind::Wikilink);
        assert_eq!(c.wiki_links[0].target, "Alpha");
        assert_eq!(c.wiki_links[1].kind, WikiKind::Embed);
        assert_eq!(c.wiki_links[1].target, "img.png");
        assert_eq!(c.wiki_links[2].anchor, Anchor::Heading("Heading".into()));
        assert_eq!(c.wiki_links[2].alias, Some("shown".into()));
        assert_eq!(c.wiki_links[3].anchor, Anchor::Block("blk".into()));
        // Each wikilink span is reserved for the base scanner.
        assert_eq!(reserved.len(), 4);
    }

    #[test]
    fn embed_pipe_is_size_not_alias() {
        let (c, _) = scan("![[img.png|100]]", 0);
        assert_eq!(c.wiki_links[0].target, "img.png");
        assert_eq!(c.wiki_links[0].alias, None);
    }

    #[test]
    fn escaped_pipe_in_table() {
        let (c, _) = scan("| [[Basic\\|Markdown]] |", 0);
        assert_eq!(c.wiki_links[0].target, "Basic");
        assert_eq!(c.wiki_links[0].alias, Some("Markdown".into()));
    }

    #[test]
    fn tags_valid_and_nested() {
        let (c, _) = scan("#y1984 #1984 #area/work", 0);
        let names: Vec<_> = c.tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["y1984", "area/work"]);
    }

    #[test]
    fn blocks_and_code_masking() {
        let (c, _) = scan("A paragraph. ^my-block\n```\n[[Fake]] #fake\n```\n", 0);
        assert_eq!(c.blocks.len(), 1);
        assert_eq!(c.blocks[0].id, "my-block");
        // Wikilinks/tags inside code are ignored.
        assert!(c.wiki_links.is_empty());
        assert!(c.tags.is_empty());
    }

    #[test]
    fn tag_normalization_lowercases() {
        assert_eq!(normalize_tag("TAG").as_deref(), Some("tag"));
        assert_eq!(normalize_tag("Foo/").as_deref(), Some("foo"));
        assert_eq!(normalize_tag("/"), None);
    }

    #[test]
    fn markdown_link_fragment_is_not_a_tag() {
        // `[text](#frag)` — the `#frag` is a link fragment, not a tag.
        let (c, _) = scan("See the [Goals section](#goals) here.", 0);
        assert!(c.tags.is_empty());
    }

    #[test]
    fn block_id_on_heading_line_is_skipped() {
        let (c, _) = scan("## A heading ^bar\n\nA paragraph. ^baz\n", 0);
        let ids: Vec<_> = c.blocks.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["baz"]); // ^bar on the heading line is not a block
    }
}
