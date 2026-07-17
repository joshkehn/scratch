//! Extraction of Obsidian-flavored Markdown constructs from a note body:
//! wikilinks, embeds, Markdown links, headings, block identifiers and tags.
//!
//! Links and tags inside fenced code blocks and inline code spans are ignored,
//! matching how Obsidian renders them. All spans are reported in byte
//! coordinates of the *whole file*, i.e. `base` (the byte offset of the body
//! after any frontmatter) is added to every offset.

use crate::model::{Anchor, BlockId, Heading, Link, LinkKind, NoteContent, Span, TagOccurrence};
use regex::Regex;
use std::sync::OnceLock;

fn wikilink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(!?)\[\[([^\[\]\n]+)\]\]").unwrap())
}

fn mdlink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(!?)\[([^\]\n]*)\]\(([^)\n]+)\)").unwrap())
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(#{1,6})\s+(.*?)\s*$").unwrap())
}

fn block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\s)(\^[A-Za-z0-9][A-Za-z0-9-]*)\s*$").unwrap())
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Preceded by start-of-text, whitespace, '(' or '>' (blockquote / callout).
    RE.get_or_init(|| Regex::new(r"(?:^|[\s>(])(#[\p{L}\p{N}_/-]+)").unwrap())
}

/// Extract everything of interest from a note body.
pub fn scan(body: &str, base: usize) -> NoteContent {
    let fenced = fenced_ranges(body);
    let inline = inline_code_ranges(body, &fenced);

    // Links first, so their spans can mask out tags (e.g. the `#` in
    // `[[Note#Heading]]` must not be read as a tag).
    let mut links = Vec::new();
    let mut link_ranges: Vec<(usize, usize)> = Vec::new();

    for caps in wikilink_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if in_ranges(m.start(), &fenced) || in_ranges(m.start(), &inline) {
            continue;
        }
        let kind = if &caps[1] == "!" {
            LinkKind::Embed
        } else {
            LinkKind::Wikilink
        };
        let (target, anchor, parsed_alias) = parse_destination(&caps[2], false);
        // For an embed, `|...` is a display size (e.g. `![[img.png|100]]`),
        // not an alias.
        let alias = if kind == LinkKind::Wikilink {
            parsed_alias
        } else {
            None
        };
        links.push(Link {
            kind,
            target,
            anchor,
            alias,
            span: Span::new(base + m.start(), m.len()),
        });
        link_ranges.push((m.start(), m.end()));
    }

    for caps in mdlink_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if in_ranges(m.start(), &fenced) || in_ranges(m.start(), &inline) {
            continue;
        }
        // Extract the destination. An angle-bracketed `<...>` destination may
        // contain spaces; a bare one ends at the first whitespace (any
        // trailing `"title"` is dropped).
        let raw_dest = caps[3].trim();
        let dest = if let Some(rest) = raw_dest.strip_prefix('<') {
            rest.split('>').next().unwrap_or(rest)
        } else {
            raw_dest.split_whitespace().next().unwrap_or("")
        };
        if dest.is_empty() || is_external(dest) {
            continue;
        }
        let kind = if &caps[1] == "!" {
            LinkKind::Embed
        } else {
            LinkKind::Markdown
        };
        // A Markdown destination has no wikilink-style `|alias`.
        let (target, anchor, _) = parse_destination(dest, true);
        links.push(Link {
            kind,
            target,
            anchor,
            alias: None,
            span: Span::new(base + m.start(), m.len()),
        });
        link_ranges.push((m.start(), m.end()));
    }

    let mut tags = Vec::new();
    for caps in tag_re().captures_iter(body) {
        let g = caps.get(1).unwrap();
        if in_ranges(g.start(), &fenced)
            || in_ranges(g.start(), &inline)
            || in_ranges(g.start(), &link_ranges)
        {
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

    let (headings, blocks) = scan_lines(body, base, &fenced);

    NoteContent {
        links,
        headings,
        blocks,
        tags,
    }
}

/// Line-based extraction of headings and block identifiers.
fn scan_lines(body: &str, base: usize, fenced: &[(usize, usize)]) -> (Vec<Heading>, Vec<BlockId>) {
    let mut headings = Vec::new();
    let mut blocks = Vec::new();
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if in_ranges(line_start, fenced) {
            continue;
        }
        let content = line.trim_end_matches('\n').trim_end_matches('\r');

        if let Some(caps) = heading_re().captures(content) {
            let level = caps[1].len() as u64;
            // Strip an optional closing sequence of '#'s.
            let text = caps[2].trim_end_matches('#').trim_end();
            if !text.is_empty() {
                let text_off = caps.get(2).unwrap().start();
                headings.push(Heading {
                    level,
                    text: text.to_string(),
                    span: Span::new(base + line_start + text_off, text.len()),
                });
            }
            continue;
        }

        if let Some(caps) = block_re().captures(content) {
            let g = caps.get(1).unwrap();
            blocks.push(BlockId {
                id: g.as_str()[1..].to_string(),
                span: Span::new(base + line_start + g.start(), g.len()),
            });
        }
    }
    (headings, blocks)
}

/// Split a link destination into a target path, an anchor, and a display
/// alias. `[[path#heading|alias]]`, `[[path#^block]]`, `[[#heading]]`.
/// The alias is the text after the first `|` (only wikilinks carry one); in
/// tables the pipe is escaped as `\|`, so the target part may keep a trailing
/// backslash. When `decode` is set (Markdown links), the path is
/// percent-decoded.
fn parse_destination(raw: &str, decode: bool) -> (String, Anchor, Option<String>) {
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
    let mut target = path.trim().to_string();
    if decode {
        target = percent_decode(&target);
    }
    (target, anchor, alias)
}

/// True if a Markdown link destination points outside the vault (has a URL
/// scheme like `http:`/`mailto:`, or a protocol-relative `//` prefix).
fn is_external(dest: &str) -> bool {
    if dest.starts_with("//") {
        return true;
    }
    // scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"
    let bytes = dest.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' {
            return i > 0;
        }
        if !(b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.') {
            return false;
        }
    }
    false
}

/// Decode `%XX` escapes in a Markdown link destination.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A tag must contain at least one non-numeric character (so `#1984` is not a
/// tag but `#y1984` is), and must be non-empty.
pub fn is_valid_tag(name: &str) -> bool {
    !name.is_empty() && name.chars().any(|c| !c.is_ascii_digit())
}

/// Byte ranges of fenced code blocks (``` / ~~~), including the fence lines.
/// An unclosed fence extends to the end of the body.
fn fenced_ranges(body: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open: Option<(u8, usize, usize)> = None; // (fence byte, count, start)
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        offset = line_end;
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        // Strip any blockquote/callout markers so fences inside callouts
        // (`> ```css`) are recognised.
        let deblock = strip_blockquote(content);
        let trimmed = deblock.trim_start();
        let indent = deblock.len() - trimmed.len();
        let fence_byte = trimmed.as_bytes().first().copied();
        let is_fence = indent <= 3
            && matches!(fence_byte, Some(b'`') | Some(b'~'))
            && trimmed
                .bytes()
                .take_while(|&b| Some(b) == fence_byte)
                .count()
                >= 3;
        if !is_fence {
            continue;
        }
        let fb = fence_byte.unwrap();
        let count = trimmed.bytes().take_while(|&b| b == fb).count();
        match open {
            None => open = Some((fb, count, line_start)),
            Some((ofb, ocount, ostart)) => {
                let rest = &trimmed[count..];
                // A closing fence matches the opening char, is at least as
                // long, and carries no info string.
                if fb == ofb && count >= ocount && rest.trim().is_empty() {
                    ranges.push((ostart, line_end));
                    open = None;
                }
            }
        }
    }
    if let Some((_, _, ostart)) = open {
        ranges.push((ostart, body.len()));
    }
    ranges
}

/// Byte ranges of inline code spans (backtick-delimited), scanned per line and
/// skipping lines already inside a fenced block.
fn inline_code_ranges(body: &str, fenced: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if in_ranges(line_start, fenced) {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                let open = i;
                let n = bytes[i..].iter().take_while(|&&b| b == b'`').count();
                i += n;
                // Find a closing run of exactly n backticks.
                let mut j = i;
                let mut closed = None;
                while j < bytes.len() {
                    if bytes[j] == b'`' {
                        let run = bytes[j..].iter().take_while(|&&b| b == b'`').count();
                        if run == n {
                            closed = Some(j + run);
                            break;
                        }
                        j += run;
                    } else {
                        j += 1;
                    }
                }
                match closed {
                    Some(end) => {
                        ranges.push((line_start + open, line_start + end));
                        i = end;
                    }
                    None => break, // no closing run on this line
                }
            } else {
                i += 1;
            }
        }
    }
    ranges
}

/// True if `pos` falls within any of the (start, end) ranges (end exclusive).
fn in_ranges(pos: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|&(s, e)| pos >= s && pos < e)
}

/// Strip leading blockquote / callout markers (`>`), each optionally preceded
/// by up to 3 spaces and followed by one space, so the remaining text can be
/// tested for a code fence. Non-blockquote indentation is preserved.
fn strip_blockquote(s: &str) -> &str {
    let mut rest = s;
    loop {
        let trimmed = rest.trim_start_matches(' ');
        if (rest.len() - trimmed.len()) <= 3 && trimmed.starts_with('>') {
            rest = &trimmed[1..];
            if let Some(r) = rest.strip_prefix(' ') {
                rest = r;
            }
        } else {
            return rest;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(nc: &NoteContent) -> Vec<(&str, LinkKind, &Anchor)> {
        nc.links
            .iter()
            .map(|l| (l.target.as_str(), l.kind, &l.anchor))
            .collect()
    }

    #[test]
    fn extracts_wikilinks_embeds_and_markdown_links() {
        let nc = scan(
            "[[Alpha]] and ![[img.png]] and [text](Beta.md) and [x](https://e.com)",
            0,
        );
        let t = targets(&nc);
        assert!(t.contains(&("Alpha", LinkKind::Wikilink, &Anchor::None)));
        assert!(t.contains(&("img.png", LinkKind::Embed, &Anchor::None)));
        assert!(t.contains(&("Beta.md", LinkKind::Markdown, &Anchor::None)));
        // The external https link is dropped.
        assert_eq!(nc.links.len(), 3);
    }

    #[test]
    fn parses_alias_and_anchors() {
        let nc = scan("[[Note#Heading|shown]] and [[Note#^blk]] and [[#Self]]", 0);
        assert_eq!(nc.links[0].target, "Note");
        assert_eq!(nc.links[0].anchor, Anchor::Heading("Heading".into()));
        assert_eq!(nc.links[0].alias, Some("shown".to_string()));
        assert_eq!(nc.links[1].anchor, Anchor::Block("blk".into()));
        assert_eq!(nc.links[1].alias, None);
        assert_eq!(nc.links[2].target, "");
        assert_eq!(nc.links[2].anchor, Anchor::Heading("Self".into()));
    }

    #[test]
    fn strips_escaped_pipe_in_tables() {
        let nc = scan("| ![[Engelbart.jpg\\|100]] | [[Basic\\|Markdown]] |", 0);
        assert_eq!(nc.links[0].target, "Engelbart.jpg");
        assert_eq!(nc.links[0].alias, None); // embed `|100` is a size, not an alias
        assert_eq!(nc.links[1].target, "Basic");
        assert_eq!(nc.links[1].alias, Some("Markdown".to_string()));
    }

    #[test]
    fn skips_external_and_angle_bracket_links() {
        let nc = scan(
            "[a](mailto:x@y.z) [b](<https://obsidian.md>) [c](//host/p)",
            0,
        );
        assert!(nc.links.is_empty());
    }

    #[test]
    fn ignores_fenced_and_inline_code() {
        let body = "Real [[A]] #real\n```\n[[Fake]] #fake\n```\n`[[Inline]] #inline`\n";
        let nc = scan(body, 0);
        let t: Vec<_> = nc.links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(t, vec!["A"]);
        let tags: Vec<_> = nc.tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(tags, vec!["real"]);
    }

    #[test]
    fn ignores_code_fence_inside_callout() {
        let body = "> [!note]\n> ```css\n> a { color: #ff0000; }\n> ```\n";
        let nc = scan(body, 0);
        assert!(
            nc.tags.is_empty(),
            "#ff0000 in a callout code block is not a tag"
        );
    }

    #[test]
    fn tag_validity_and_nesting() {
        let nc = scan("#y1984 #1984 #area/work end", 0);
        let tags: Vec<_> = nc.tags.iter().map(|t| t.name.as_str()).collect();
        // #1984 is numeric-only and rejected; nested tag keeps its slash.
        assert_eq!(tags, vec!["y1984", "area/work"]);
    }

    #[test]
    fn headings_have_levels_and_clean_text() {
        let nc = scan("# Title\n## Sub ##\ntext\n### Deep\n", 0);
        let hs: Vec<_> = nc
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str()))
            .collect();
        assert_eq!(hs, vec![(1, "Title"), (2, "Sub"), (3, "Deep")]);
    }

    #[test]
    fn block_ids_are_extracted() {
        let nc = scan("A paragraph. ^my-block\n", 0);
        assert_eq!(nc.blocks.len(), 1);
        assert_eq!(nc.blocks[0].id, "my-block");
    }

    #[test]
    fn spans_are_byte_accurate_with_base_offset() {
        let body = "see [[Target]] here";
        let nc = scan(body, 100);
        let span = nc.links[0].span;
        assert_eq!(span.start, 100 + 4);
        assert_eq!(
            &body[span.start - 100..span.start - 100 + span.length],
            "[[Target]]"
        );
    }

    #[test]
    fn tag_after_wikilink_anchor_is_not_double_counted() {
        // The '#Heading' inside the wikilink must not be read as a tag.
        let nc = scan("[[Note#Heading]]", 0);
        assert!(nc.tags.is_empty());
    }
}
