//! Extraction of Obsidian-flavored Markdown constructs from a note body:
//! wikilinks, embeds, Markdown links (inline and reference-style), headings,
//! block identifiers, tags, fenced code blocks, task items and inline HTML.
//!
//! Links and tags inside fenced code blocks and inline code spans are ignored,
//! matching how Obsidian renders them. All spans are reported in byte
//! coordinates of the *whole file*, i.e. `base` (the byte offset of the body
//! after any frontmatter) is added to every offset.

use crate::model::{
    Anchor, BlockId, CodeBlock, Heading, HtmlAttr, HtmlElement, Link, LinkKind, NoteContent, Span,
    TagOccurrence, Todo,
};
use regex::Regex;
use std::collections::HashMap;
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

/// A link reference definition line: `[label]: destination "optional title"`.
fn linkdef_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"^ {0,3}\[([^\]\n]+)\]:\s+(\S+)"#).unwrap())
}

/// A full/collapsed reference link usage: `[text][label]` or `[label][]`.
fn reflink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\[\]\n]*)\]\[([^\[\]\n]*)\]").unwrap())
}

/// A bracketed span `[label]`, used to find shortcut reference links.
fn shortcut_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\[\]\n]+)\]").unwrap())
}

/// A task-list item: `- [ ] text` / `- [x] text` (also `*`/`+` markers).
fn todo_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*[-*+]\s+\[(.)\]\s+(.*\S)\s*$").unwrap())
}

/// An inline HTML opening or self-closing tag (quotes may contain `>`). The
/// name excludes `:` so scheme autolinks like `<http://x>` are not matched.
fn html_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<([a-zA-Z][a-zA-Z0-9-]*)((?:[^>"']|"[^"]*"|'[^']*')*)>"#).unwrap()
    })
}

/// One HTML attribute: `name`, `name=value`, `name="value"`, `name='value'`.
fn html_attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"([a-zA-Z_:][-a-zA-Z0-9_:.]*)(?:\s*=\s*("[^"]*"|'[^']*'|[^\s"'=<>`]+))?"#)
            .unwrap()
    })
}

/// Extract everything of interest from a note body.
pub fn scan(body: &str, base: usize) -> NoteContent {
    let code = fenced_blocks(body);
    let fenced: Vec<(usize, usize)> = code.iter().map(|b| (b.start, b.end)).collect();
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

    // Reference-style Markdown links (`[text][ref]`, `[ref][]`, `[ref]`), using
    // the link reference definitions collected from the whole document.
    let (defs, def_ranges) = link_definitions(body, &fenced);
    for link in reference_links(
        body,
        base,
        &defs,
        &fenced,
        &inline,
        &def_ranges,
        &link_ranges,
    ) {
        link_ranges.push((
            link.span.start - base,
            link.span.start - base + link.span.length,
        ));
        links.push(link);
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

    let (headings, blocks, todos) = scan_lines(body, base, &fenced);

    let code_blocks = code
        .iter()
        .map(|b| CodeBlock {
            language: b.language.clone(),
            span: Span::new(base + b.start, b.end - b.start),
        })
        .collect();

    let html = scan_html(body, base, &fenced, &inline);

    NoteContent {
        links,
        headings,
        blocks,
        tags,
        code_blocks,
        todos,
        html,
    }
}

/// Line-based extraction of headings, task items and block identifiers.
fn scan_lines(
    body: &str,
    base: usize,
    fenced: &[(usize, usize)],
) -> (Vec<Heading>, Vec<BlockId>, Vec<Todo>) {
    let mut headings = Vec::new();
    let mut blocks = Vec::new();
    let mut todos = Vec::new();
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

        if let Some(caps) = todo_re().captures(content) {
            // The span covers the whole item line, so links/tags on the line
            // can be attributed to this task by containment.
            todos.push(Todo {
                checked: &caps[1] != " ",
                text: caps[2].trim().to_string(),
                span: Span::new(base + line_start, content.len()),
            });
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
    (headings, blocks, todos)
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

/// Collect link reference definitions (`[label]: dest`), returning a map from
/// normalized label to destination plus the byte ranges of the definition
/// lines. Footnote definitions (`[^label]: ...`) are ignored.
fn link_definitions(
    body: &str,
    fenced: &[(usize, usize)],
) -> (HashMap<String, String>, Vec<(usize, usize)>) {
    let mut defs = HashMap::new();
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        offset = line_end;
        if in_ranges(line_start, fenced) {
            continue;
        }
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        if let Some(caps) = linkdef_re().captures(content) {
            let label = &caps[1];
            if label.starts_with('^') {
                continue; // footnote definition, not a link definition
            }
            defs.entry(normalize_label(label))
                .or_insert_with(|| caps[2].to_string());
            ranges.push((line_start, line_end));
        }
    }
    (defs, ranges)
}

/// Normalize a reference label the way CommonMark does: trim, collapse internal
/// whitespace, and lower-case.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Resolve a reference-style link usage against the definitions into a `Link`,
/// or `None` when the label is undefined or the destination is external.
fn make_ref_link(
    defs: &HashMap<String, String>,
    label: &str,
    base: usize,
    m: &regex::Match,
) -> Option<Link> {
    let dest = defs.get(&normalize_label(label))?;
    let raw = dest.trim();
    let d = raw
        .strip_prefix('<')
        .map(|r| r.split('>').next().unwrap_or(r))
        .unwrap_or(raw);
    if d.is_empty() || is_external(d) {
        return None;
    }
    let (target, anchor, _) = parse_destination(d, true);
    Some(Link {
        kind: LinkKind::Markdown,
        target,
        anchor,
        alias: None,
        span: Span::new(base + m.start(), m.len()),
    })
}

/// Reference-style Markdown links: full (`[text][label]`), collapsed
/// (`[label][]`) and shortcut (`[label]`), each resolved against `defs`.
fn reference_links(
    body: &str,
    base: usize,
    defs: &HashMap<String, String>,
    fenced: &[(usize, usize)],
    inline: &[(usize, usize)],
    def_ranges: &[(usize, usize)],
    link_ranges: &[(usize, usize)],
) -> Vec<Link> {
    if defs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut used: Vec<(usize, usize)> = link_ranges.to_vec();
    let masked = |pos: usize, used: &[(usize, usize)]| {
        in_ranges(pos, fenced)
            || in_ranges(pos, inline)
            || in_ranges(pos, def_ranges)
            || in_ranges(pos, used)
    };

    // Full and collapsed: `[text][label]` / `[label][]`.
    for caps in reflink_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masked(m.start(), &used) {
            continue;
        }
        let label = if caps[2].trim().is_empty() {
            &caps[1]
        } else {
            &caps[2]
        };
        if let Some(link) = make_ref_link(defs, label, base, &m) {
            used.push((m.start(), m.end()));
            out.push(link);
        }
    }

    // Shortcut: `[label]`, only where a definition exists and it is not part of
    // a wikilink, image, inline link, full reference, or a definition line.
    for caps in shortcut_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masked(m.start(), &used) {
            continue;
        }
        let prev = body[..m.start()].chars().next_back();
        let next = body[m.end()..].chars().next();
        if matches!(prev, Some('[') | Some('!'))
            || matches!(next, Some('(') | Some('[') | Some(':'))
        {
            continue;
        }
        let label = &caps[1];
        if label.starts_with('^') {
            continue; // footnote reference
        }
        if let Some(link) = make_ref_link(defs, label, base, &m) {
            used.push((m.start(), m.end()));
            out.push(link);
        }
    }
    out
}

/// Inline HTML element occurrences (opening / self-closing tags), skipping
/// code. Closing tags (`</div>`) and scheme/email autolinks are not matched.
fn scan_html(
    body: &str,
    base: usize,
    fenced: &[(usize, usize)],
    inline: &[(usize, usize)],
) -> Vec<HtmlElement> {
    let mut out = Vec::new();
    for caps in html_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if in_ranges(m.start(), fenced) || in_ranges(m.start(), inline) {
            continue;
        }
        // A real tag's attribute region is empty, or starts with whitespace or
        // a self-closing '/'. Anything else (e.g. `<foo@bar.com>`) is not HTML.
        let blob = &caps[2];
        if let Some(c) = blob.chars().next() {
            if !c.is_whitespace() && c != '/' {
                continue;
            }
        }
        let name = caps[1].to_lowercase();
        let mut attrs = Vec::new();
        for a in html_attr_re().captures_iter(blob.trim_end_matches('/')) {
            let attr_name = a[1].to_lowercase();
            let value = a
                .get(2)
                .map(|v| strip_quotes(v.as_str()))
                .unwrap_or_default();
            attrs.push(HtmlAttr {
                name: attr_name,
                value,
            });
        }
        out.push(HtmlElement {
            name,
            span: Span::new(base + m.start(), m.len()),
            attrs,
        });
    }
    out
}

/// Strip matching surrounding single/double quotes from an attribute value.
fn strip_quotes(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// A fenced code block: its byte range (fences included) and info-string
/// language, lower-cased.
struct FencedBlock {
    start: usize,
    end: usize,
    language: String,
}

/// Fenced code blocks (``` / ~~~). An unclosed fence extends to end of body.
fn fenced_blocks(body: &str) -> Vec<FencedBlock> {
    let mut blocks = Vec::new();
    // (fence byte, count, start offset, language)
    let mut open: Option<(u8, usize, usize, String)> = None;
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
        match &open {
            None => {
                let language = trimmed[count..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                open = Some((fb, count, line_start, language));
            }
            Some((ofb, ocount, ostart, language)) => {
                let rest = &trimmed[count..];
                // A closing fence matches the opening char, is at least as
                // long, and carries no info string.
                if fb == *ofb && count >= *ocount && rest.trim().is_empty() {
                    blocks.push(FencedBlock {
                        start: *ostart,
                        end: line_end,
                        language: language.clone(),
                    });
                    open = None;
                }
            }
        }
    }
    if let Some((_, _, ostart, language)) = open {
        blocks.push(FencedBlock {
            start: ostart,
            end: body.len(),
            language,
        });
    }
    blocks
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

    #[test]
    fn reference_style_links_full_collapsed_shortcut() {
        let body = "See [text][ref], [ref2][], and [ref3].\n\n\
                    [ref]: Target\n[ref2]: Other\n[ref3]: Third\n";
        let nc = scan(body, 0);
        let targets: Vec<_> = nc.links.iter().map(|l| l.target.as_str()).collect();
        assert!(targets.contains(&"Target"));
        assert!(targets.contains(&"Other"));
        assert!(targets.contains(&"Third"));
        assert!(nc.links.iter().all(|l| l.kind == LinkKind::Markdown));
    }

    #[test]
    fn footnotes_are_not_links() {
        let nc = scan("A claim.[^1]\n\n[^1]: the footnote text.\n", 0);
        assert!(nc.links.is_empty());
    }

    #[test]
    fn external_reference_definition_is_skipped() {
        let nc = scan("Read [more][x].\n\n[x]: https://example.com\n", 0);
        assert!(nc.links.is_empty());
    }

    #[test]
    fn code_block_language_and_plain() {
        let nc = scan("```php\n<?php echo 1;\n```\n\n```\nplain\n```\n", 0);
        let langs: Vec<_> = nc.code_blocks.iter().map(|c| c.language.as_str()).collect();
        assert_eq!(langs, vec!["php", ""]);
    }

    #[test]
    fn todos_checked_and_unchecked() {
        let nc = scan("- [ ] open\n- [x] done\n- not a task\n", 0);
        assert_eq!(nc.todos.len(), 2);
        assert!(!nc.todos[0].checked);
        assert!(nc.todos[1].checked);
        assert_eq!(nc.todos[0].text, "open");
    }

    #[test]
    fn html_elements_and_attributes_parsed() {
        let nc = scan(
            r#"<span style="color: red" class="x">hi</span> and <br>"#,
            0,
        );
        assert_eq!(nc.html.len(), 2);
        let span = nc.html.iter().find(|e| e.name == "span").unwrap();
        assert!(span
            .attrs
            .iter()
            .any(|a| a.name == "style" && a.value == "color: red"));
        assert!(nc.html.iter().any(|e| e.name == "br" && e.attrs.is_empty()));
    }

    #[test]
    fn autolinks_are_not_html_elements() {
        let nc = scan("Visit <https://example.com> or email <a@b.com>.", 0);
        assert!(nc.html.is_empty());
    }

    #[test]
    fn code_fence_masks_html_and_tasks() {
        let nc = scan("```html\n<div style=\"x\">y</div>\n- [ ] fake\n```\n", 0);
        assert!(nc.html.is_empty());
        assert!(nc.todos.is_empty());
        assert_eq!(nc.code_blocks.len(), 1);
        assert_eq!(nc.code_blocks[0].language, "html");
    }
}
