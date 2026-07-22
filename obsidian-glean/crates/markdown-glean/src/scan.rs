//! The generic Markdown scanner (CommonMark + GFM).
//!
//! `scan` extracts headings, links/images, code blocks, inline HTML, task
//! items, tables, footnotes and link reference definitions from a document
//! body. Constructs inside fenced code blocks and inline code are ignored.
//!
//! A dialect passes `reserved` byte ranges (e.g. Obsidian `[[wikilinks]]`) that
//! it owns; the generic scanner will not parse links/HTML inside them. All the
//! lexical helpers (`CodeMasks`, `is_external`, `percent_decode`,
//! `strip_blockquote`, `slugify`) are public so dialect crates can reuse them.

use crate::model::{
    CodeBlock, Footnote, Heading, HeadingKind, HtmlAttr, HtmlElement, Link, LinkKind, LinkRefDef,
    MarkdownContent, Span, Table,
};
use crate::table;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// (definitions, normalized-label -> destination, definition line ranges).
type LinkDefs = (
    Vec<LinkRefDef>,
    HashMap<String, String>,
    Vec<(usize, usize)>,
);

// ---------------------------------------------------------------------------
// Regexes
// ---------------------------------------------------------------------------

fn inline_link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(!?)\[([^\]\n]*)\]\(([^)\n]*)\)").unwrap())
}

fn reflink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(!?)\[([^\[\]\n]*)\]\[([^\[\]\n]*)\]").unwrap())
}

fn shortcut_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(!?)\[([^\[\]\n]+)\]").unwrap())
}

fn autolink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // <scheme:...> or <email>
    RE.get_or_init(|| {
        Regex::new(r"<([a-zA-Z][a-zA-Z0-9+.-]{1,31}:[^<>\s]*|[^<>\s@]+@[^<>\s]+\.[^<>\s]+)>")
            .unwrap()
    })
}

fn bare_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"https?://[^\s<>()\[\]]+").unwrap())
}

fn linkdef_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ {0,3}\[([^\]\n]+)\]:\s+(\S+)").unwrap())
}

fn footnote_def_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ {0,3}\[\^([^\]\n]+)\]:").unwrap())
}

fn footnote_ref_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\^([^\]\n]+)\]").unwrap())
}

fn atx_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ {0,3}(#{1,6})\s+(.*?)\s*$").unwrap())
}

fn setext_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ {0,3}(=+|-+)\s*$").unwrap())
}

fn task_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*[-*+]\s+\[(.)\]\s+(.*\S)\s*$").unwrap())
}

fn html_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"<([a-zA-Z][a-zA-Z0-9-]*)((?:[^>"']|"[^"]*"|'[^']*')*)>"#).unwrap()
    })
}

fn html_attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"([a-zA-Z_:][-a-zA-Z0-9_:.]*)(?:\s*=\s*("[^"]*"|'[^']*'|[^\s"'=<>`]+))?"#)
            .unwrap()
    })
}

// ---------------------------------------------------------------------------
// Code masks
// ---------------------------------------------------------------------------

/// Byte ranges occupied by fenced code blocks and inline code spans.
#[derive(Debug, Clone, Default)]
pub struct CodeMasks {
    pub fenced: Vec<(usize, usize)>,
    pub inline: Vec<(usize, usize)>,
}

impl CodeMasks {
    /// True if `pos` is inside any code span.
    pub fn contains(&self, pos: usize) -> bool {
        in_ranges(pos, &self.fenced) || in_ranges(pos, &self.inline)
    }
}

/// A fenced code block with its byte range and info string.
pub struct FencedBlock {
    pub start: usize,
    pub end: usize,
    pub language: String,
    pub info: String,
}

/// Compute code masks and the list of fenced blocks for a body.
pub fn compute_masks(body: &str) -> (CodeMasks, Vec<FencedBlock>) {
    let fenced_blocks = fenced_blocks(body);
    let fenced: Vec<(usize, usize)> = fenced_blocks.iter().map(|b| (b.start, b.end)).collect();
    let inline = inline_code_ranges(body, &fenced);
    (CodeMasks { fenced, inline }, fenced_blocks)
}

// ---------------------------------------------------------------------------
// Top-level scan
// ---------------------------------------------------------------------------

/// Extract generic Markdown constructs from `body`. Spans are reported in
/// whole-file coordinates (i.e. `base` is added). `reserved` byte ranges (in
/// body coordinates) are treated as owned by a dialect and are not parsed for
/// generic links/HTML.
pub fn scan(body: &str, base: usize, reserved: &[(usize, usize)]) -> MarkdownContent {
    let (masks, fenced_blocks) = compute_masks(body);

    let code_blocks = fenced_blocks
        .iter()
        .map(|b| CodeBlock {
            language: b.language.clone(),
            info: b.info.clone(),
            span: Span::new(base + b.start, b.end - b.start),
        })
        .collect();

    let (links, link_defs) = scan_links(body, base, &masks, reserved);
    let html = scan_html(body, base, &masks, reserved);
    let (headings, tasks, tables) = scan_lines(body, base, &masks, reserved);
    let (footnote_defs, footnote_refs) = scan_footnotes(body, base, &masks, reserved);

    MarkdownContent {
        headings,
        links,
        link_defs,
        code_blocks,
        html,
        tasks,
        tables,
        footnote_defs,
        footnote_refs,
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

fn scan_links(
    body: &str,
    base: usize,
    masks: &CodeMasks,
    reserved: &[(usize, usize)],
) -> (Vec<Link>, Vec<LinkRefDef>) {
    let (defs, def_map, def_ranges) = link_definitions(body, base, masks);

    let mut links = Vec::new();
    let mut used: Vec<(usize, usize)> = Vec::new();

    let masked = |pos: usize, used: &[(usize, usize)]| {
        masks.contains(pos)
            || in_ranges(pos, reserved)
            || in_ranges(pos, &def_ranges)
            || in_ranges(pos, used)
    };

    // Inline links/images: [text](dest) and ![alt](dest).
    for caps in inline_link_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masked(m.start(), &used) {
            continue;
        }
        let image = &caps[1] == "!";
        let dest = clean_dest(&caps[3]);
        if dest.is_empty() {
            continue;
        }
        links.push(Link {
            kind: LinkKind::Inline,
            image,
            dest,
            text: caps[2].to_string(),
            span: Span::new(base + m.start(), m.len()),
        });
        used.push((m.start(), m.end()));
    }

    // Full / collapsed reference links: [text][label] / [label][].
    for caps in reflink_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masked(m.start(), &used) {
            continue;
        }
        let image = &caps[1] == "!";
        let text = &caps[2];
        let (kind, label) = if caps[3].trim().is_empty() {
            (LinkKind::Collapsed, text)
        } else {
            (LinkKind::Reference, &caps[3])
        };
        if let Some(dest) = def_map.get(&normalize_label(label)) {
            links.push(Link {
                kind,
                image,
                dest: dest.clone(),
                text: text.to_string(),
                span: Span::new(base + m.start(), m.len()),
            });
            used.push((m.start(), m.end()));
        }
    }

    // Shortcut reference links: [label] (only if defined).
    for caps in shortcut_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masked(m.start(), &used) {
            continue;
        }
        let image = &caps[1] == "!";
        let start = m.start() + caps[1].len(); // position of '['
        let prev = body[..start].chars().next_back();
        let next = body[m.end()..].chars().next();
        if prev == Some('[') || matches!(next, Some('(') | Some('[') | Some(':')) {
            continue;
        }
        let label = &caps[2];
        if label.starts_with('^') {
            continue; // footnote reference
        }
        if let Some(dest) = def_map.get(&normalize_label(label)) {
            links.push(Link {
                kind: LinkKind::Shortcut,
                image,
                dest: dest.clone(),
                text: label.to_string(),
                span: Span::new(base + m.start(), m.len()),
            });
            used.push((m.start(), m.end()));
        }
    }

    // Autolinks: <scheme:...> / <email>.
    for caps in autolink_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masked(m.start(), &used) {
            continue;
        }
        let url = caps[1].to_string();
        links.push(Link {
            kind: LinkKind::Autolink,
            image: false,
            dest: url.clone(),
            text: url,
            span: Span::new(base + m.start(), m.len()),
        });
        used.push((m.start(), m.end()));
    }

    // Bare URL autolink literals (GFM): http(s)://...
    for m in bare_url_re().find_iter(body) {
        if masked(m.start(), &used) {
            continue;
        }
        let url = m
            .as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'']);
        if url.is_empty() {
            continue;
        }
        links.push(Link {
            kind: LinkKind::Autolink,
            image: false,
            dest: url.to_string(),
            text: url.to_string(),
            span: Span::new(base + m.start(), url.len()),
        });
        used.push((m.start(), m.start() + url.len()));
    }

    (links, defs)
}

/// Clean an inline link destination: strip surrounding `<>` and an optional
/// trailing `"title"`.
fn clean_dest(raw: &str) -> String {
    let raw = raw.trim();
    let dest = if let Some(rest) = raw.strip_prefix('<') {
        rest.split('>').next().unwrap_or(rest)
    } else {
        raw.split_whitespace().next().unwrap_or("")
    };
    dest.to_string()
}

/// Collect link reference definitions. Returns the fact list, a normalized
/// label -> destination map, and the byte ranges of definition lines.
fn link_definitions(body: &str, base: usize, masks: &CodeMasks) -> LinkDefs {
    let mut defs = Vec::new();
    let mut map = HashMap::new();
    let mut ranges = Vec::new();
    for line in lines(body) {
        if masks.contains(line.start) {
            continue;
        }
        if let Some(caps) = linkdef_re().captures(line.text) {
            let label = &caps[1];
            if label.starts_with('^') {
                continue; // footnote definition, not a link definition
            }
            let dest = clean_dest(&caps[2]);
            let norm = normalize_label(label);
            map.entry(norm).or_insert_with(|| dest.clone());
            defs.push(LinkRefDef {
                label: label.trim().to_string(),
                destination: dest,
                span: Span::new(base + line.start, line.text.len()),
            });
            ranges.push((line.start, line.end));
        }
    }
    (defs, map, ranges)
}

/// CommonMark reference-label normalization: trim, collapse internal
/// whitespace, and case-fold.
pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True if a destination points outside the corpus (URL scheme or `//` prefix).
pub fn is_external(dest: &str) -> bool {
    if dest.starts_with("//") {
        return true;
    }
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

/// The URL scheme of an external destination (e.g. "https"), or "".
pub fn url_scheme(dest: &str) -> String {
    if let Some(i) = dest.find(':') {
        if dest[..i]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        {
            return dest[..i].to_lowercase();
        }
    }
    if dest.starts_with("//") {
        return String::new();
    }
    String::new()
}

/// Decode `%XX` escapes in a Markdown link destination.
pub fn percent_decode(s: &str) -> String {
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

// ---------------------------------------------------------------------------
// Line-based: headings (ATX + Setext), tasks, tables
// ---------------------------------------------------------------------------

/// A logical source line and its byte range (text excludes the newline).
pub struct SrcLine<'a> {
    pub start: usize,
    pub end: usize,
    pub text: &'a str,
}

/// Split a body into lines with byte offsets.
pub fn lines(body: &str) -> Vec<SrcLine<'_>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let start = offset;
        let end = offset + line.len();
        offset = end;
        let text = line.trim_end_matches('\n').trim_end_matches('\r');
        out.push(SrcLine { start, end, text });
    }
    out
}

#[allow(clippy::type_complexity)]
fn scan_lines(
    body: &str,
    base: usize,
    masks: &CodeMasks,
    reserved: &[(usize, usize)],
) -> (Vec<Heading>, Vec<crate::model::TaskItem>, Vec<Table>) {
    let lines = lines(body);
    let mut headings = Vec::new();
    let mut tasks = Vec::new();
    let mut tables = Vec::new();
    let mut consumed_until = 0usize; // byte offset consumed by a table

    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if masks.contains(line.start)
            || in_ranges(line.start, reserved)
            || line.start < consumed_until
        {
            i += 1;
            continue;
        }

        // GFM table: header row + delimiter row.
        if i + 1 < lines.len() && !masks.contains(lines[i + 1].start) {
            if let Some(t) = table::try_table(&lines, i, base) {
                consumed_until = t.table.span.end() - base;
                i = t.next_index;
                tables.push(t.table);
                continue;
            }
        }

        // ATX heading.
        if let Some(caps) = atx_re().captures(line.text) {
            let level = caps[1].len() as u64;
            let text = caps[2].trim_end_matches('#').trim_end();
            if !text.is_empty() {
                let off = caps.get(2).unwrap().start();
                headings.push(Heading {
                    text: text.to_string(),
                    level,
                    slug: slugify(text),
                    kind: HeadingKind::Atx,
                    span: Span::new(base + line.start + off, text.len()),
                });
            }
            i += 1;
            continue;
        }

        // Setext heading: a paragraph line followed by an `===`/`---` underline.
        if i + 1 < lines.len() {
            let next = &lines[i + 1];
            if !masks.contains(next.start) && is_paragraph(line.text) {
                if let Some(caps) = setext_re().captures(next.text) {
                    let level = if caps[1].starts_with('=') { 1 } else { 2 };
                    let text = line.text.trim();
                    headings.push(Heading {
                        text: text.to_string(),
                        level,
                        slug: slugify(text),
                        kind: HeadingKind::Setext,
                        span: Span::new(base + line.start, line.text.len()),
                    });
                    i += 2;
                    continue;
                }
            }
        }

        // Task list item.
        if let Some(caps) = task_re().captures(line.text) {
            tasks.push(crate::model::TaskItem {
                checked: &caps[1] != " ",
                marker: caps[1].to_string(),
                text: caps[2].trim().to_string(),
                span: Span::new(base + line.start, line.text.len()),
            });
        }

        i += 1;
    }

    (headings, tasks, tables)
}

/// A line that can be the text of a Setext heading: non-blank and not itself a
/// block starter (heading, list, quote, fence, thematic break, HTML block, or
/// a link-reference / footnote definition — which the reference parser strips
/// before paragraph handling in CommonMark).
fn is_paragraph(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let first = t.as_bytes()[0];
    // Exclude obvious block starters (a leading list marker `-` is covered here)
    // and HTML-block / definition lines.
    if matches!(
        first,
        b'#' | b'>' | b'-' | b'*' | b'+' | b'=' | b'`' | b'~' | b'|' | b'<'
    ) {
        return false;
    }
    if linkdef_re().is_match(text) || footnote_def_re().is_match(text) {
        return false;
    }
    // An ordered-list item like "1. text" is not paragraph text.
    !(first.is_ascii_digit() && t.contains(". "))
}

/// A GitHub-style anchor slug: lower-case, spaces to hyphens, drop other
/// punctuation, collapse repeats.
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_hyphen = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            prev_hyphen = false;
        } else if (c == ' ' || c == '-' || c == '_') && !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
        // else: drop punctuation
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

// ---------------------------------------------------------------------------
// Footnotes
// ---------------------------------------------------------------------------

fn scan_footnotes(
    body: &str,
    base: usize,
    masks: &CodeMasks,
    reserved: &[(usize, usize)],
) -> (Vec<Footnote>, Vec<Footnote>) {
    let mut defs = Vec::new();
    // Suppress only the leading `[^label]:` token of a definition line, so a
    // footnote reference elsewhere on that line is still captured.
    let mut def_tokens = Vec::new();
    for line in lines(body) {
        if masks.contains(line.start) {
            continue;
        }
        if let Some(caps) = footnote_def_re().captures(line.text) {
            defs.push(Footnote {
                label: caps[1].trim().to_string(),
                span: Span::new(base + line.start, line.text.len()),
            });
            def_tokens.push((line.start, line.start + caps.get(0).unwrap().end()));
        }
    }

    let mut refs = Vec::new();
    for caps in footnote_ref_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masks.contains(m.start())
            || in_ranges(m.start(), &def_tokens)
            || in_ranges(m.start(), reserved)
        {
            continue;
        }
        // A `[^x]` immediately followed by ':' is a definition, not a reference.
        if body[m.end()..].starts_with(':') {
            continue;
        }
        refs.push(Footnote {
            label: caps[1].trim().to_string(),
            span: Span::new(base + m.start(), m.len()),
        });
    }
    (defs, refs)
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

fn scan_html(
    body: &str,
    base: usize,
    masks: &CodeMasks,
    reserved: &[(usize, usize)],
) -> Vec<HtmlElement> {
    let mut out = Vec::new();
    for caps in html_re().captures_iter(body) {
        let m = caps.get(0).unwrap();
        if masks.contains(m.start()) || in_ranges(m.start(), reserved) {
            continue;
        }
        let blob = &caps[2];
        if let Some(c) = blob.chars().next() {
            if !c.is_whitespace() && c != '/' {
                continue; // autolink like <http://...> or <a@b>, not an element
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

fn strip_quotes(s: &str) -> String {
    let b = s.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Fenced / inline code masking
// ---------------------------------------------------------------------------

fn fenced_blocks(body: &str) -> Vec<FencedBlock> {
    let mut blocks = Vec::new();
    // (fence byte, count, start, language, info)
    let mut open: Option<(u8, usize, usize, String, String)> = None;
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        offset = line_end;
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
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
                let info = trimmed[count..].trim().to_string();
                let language = info.split_whitespace().next().unwrap_or("").to_lowercase();
                open = Some((fb, count, line_start, language, info));
            }
            Some((ofb, ocount, ostart, language, info)) => {
                let rest = &trimmed[count..];
                if fb == *ofb && count >= *ocount && rest.trim().is_empty() {
                    blocks.push(FencedBlock {
                        start: *ostart,
                        end: line_end,
                        language: language.clone(),
                        info: info.clone(),
                    });
                    open = None;
                }
            }
        }
    }
    if let Some((_, _, ostart, language, info)) = open {
        blocks.push(FencedBlock {
            start: ostart,
            end: body.len(),
            language,
            info,
        });
    }
    blocks
}

/// Byte ranges of inline code spans, scanned over the whole body so that
/// multi-line spans (valid in CommonMark) are masked. A span is a run of `n`
/// backticks, content, then a matching run of exactly `n` backticks; spans do
/// not start inside a fenced block.
fn inline_code_ranges(body: &str, fenced: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let bytes = body.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' || in_ranges(i, fenced) {
            i += 1;
            continue;
        }
        let open = i;
        let n = bytes[i..].iter().take_while(|&&b| b == b'`').count();
        i += n;
        // Find a closing run of exactly `n` backticks (may cross newlines).
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
        if let Some(end) = closed {
            ranges.push((open, end));
            i = end;
        }
        // Unclosed run: the backticks are literal; `i` is already past them.
    }
    ranges
}

/// Strip leading blockquote/callout markers (`>`) so that fences inside
/// blockquotes and callouts are recognised; other indentation is preserved.
pub fn strip_blockquote(s: &str) -> &str {
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

/// True if `pos` falls within any (start, end) range (end exclusive).
pub fn in_ranges(pos: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|&(s, e)| pos >= s && pos < e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ColumnAlign;

    fn c(body: &str) -> MarkdownContent {
        scan(body, 0, &[])
    }

    #[test]
    fn atx_and_setext_headings() {
        let m = c("# Title\n\nSub\n===\n\nOther\n---\n");
        let hs: Vec<_> = m
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.kind))
            .collect();
        assert!(hs.contains(&(1, "Title", HeadingKind::Atx)));
        assert!(hs.contains(&(1, "Sub", HeadingKind::Setext)));
        assert!(hs.contains(&(2, "Other", HeadingKind::Setext)));
    }

    #[test]
    fn heading_slug() {
        let m = c("## Hello, World! & more\n");
        assert_eq!(m.headings[0].slug, "hello-world-more");
    }

    #[test]
    fn gfm_table_columns_and_alignment() {
        let m = c("| A | B | C |\n|:--|--:|:-:|\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n");
        assert_eq!(m.tables.len(), 1);
        let t = &m.tables[0];
        assert_eq!(t.columns.len(), 3);
        assert_eq!(t.rows, 2);
        assert_eq!(t.columns[0].align, ColumnAlign::Left);
        assert_eq!(t.columns[1].align, ColumnAlign::Right);
        assert_eq!(t.columns[2].align, ColumnAlign::Center);
        assert_eq!(t.columns[0].header, "A");
    }

    #[test]
    fn lone_pipe_line_is_not_a_table() {
        let m = c("| just some | text without a delimiter row |\n\npara\n");
        assert!(m.tables.is_empty());
    }

    #[test]
    fn footnote_definition_and_reference() {
        let m = c("A claim.[^note]\n\n[^note]: the definition.\n");
        assert_eq!(m.footnote_refs.len(), 1);
        assert_eq!(m.footnote_refs[0].label, "note");
        assert_eq!(m.footnote_defs.len(), 1);
        assert_eq!(m.footnote_defs[0].label, "note");
    }

    #[test]
    fn links_inline_reference_autolink_bare_image() {
        let m =
            c("[a](x.md) ![img](y.png) [r][ref] <https://e.com> http://bare.com\n\n[ref]: z.md\n");
        let d: Vec<_> = m
            .links
            .iter()
            .map(|l| (l.dest.as_str(), l.kind, l.image))
            .collect();
        assert!(d.contains(&("x.md", LinkKind::Inline, false)));
        assert!(d.contains(&("y.png", LinkKind::Inline, true)));
        assert!(d.contains(&("z.md", LinkKind::Reference, false)));
        assert!(d
            .iter()
            .any(|(x, k, _)| *k == LinkKind::Autolink && x.contains("e.com")));
        assert!(d
            .iter()
            .any(|(x, k, _)| *k == LinkKind::Autolink && *x == "http://bare.com"));
        assert_eq!(m.link_defs.len(), 1);
        assert_eq!(m.link_defs[0].destination, "z.md");
    }

    #[test]
    fn code_masks_links_headings_and_tables() {
        let m = c("real [a](x)\n`[b](y)`\n```\n[c](z)\n# NotHeading\n```\n");
        let dests: Vec<_> = m.links.iter().map(|l| l.dest.as_str()).collect();
        assert_eq!(dests, vec!["x"]);
        assert_eq!(m.code_blocks.len(), 1);
        assert!(!m.headings.iter().any(|h| h.text == "NotHeading"));
    }

    #[test]
    fn reserved_spans_skip_generic_parsing() {
        // A dialect reserving the first link means the base won't parse it.
        let body = "[a](x) [b](y)";
        let m = scan(body, 0, &[(0, 6)]);
        let dests: Vec<_> = m.links.iter().map(|l| l.dest.as_str()).collect();
        assert_eq!(dests, vec!["y"]);
    }

    #[test]
    fn email_autolink_captured() {
        let m = c("mail <a@b.com> here");
        assert!(m
            .links
            .iter()
            .any(|l| l.kind == LinkKind::Autolink && l.dest == "a@b.com"));
    }

    // --- regression tests for reviewed defects ---

    #[test]
    fn mismatched_table_is_rejected_and_setext_survives() {
        // Header (2 cells) vs delimiter (1 cell): not a GFM table.
        assert!(c("| a | b |\n| --- |\nx\n").tables.is_empty());
        // A pipe-containing paragraph + dash underline is a Setext heading,
        // not a phantom table.
        let m = c("a | b\n-----\n");
        assert!(m.tables.is_empty());
        assert!(m
            .headings
            .iter()
            .any(|h| h.text == "a | b" && h.kind == HeadingKind::Setext));
    }

    #[test]
    fn multiline_inline_code_is_masked() {
        // A code span crossing a line: the interior link must not be extracted.
        let m = c("start `x\n[a](b)` end\n");
        assert!(m.links.is_empty());
    }

    #[test]
    fn link_ref_definition_is_not_a_setext_heading() {
        let m = c("[foo]: /url\n===\n");
        assert!(!m.headings.iter().any(|h| h.kind == HeadingKind::Setext));
        assert_eq!(m.link_defs.len(), 1);
    }

    #[test]
    fn footnote_reference_on_definition_line_is_captured() {
        let m = c("[^1]: see [^2] for details.\n");
        let labels: Vec<_> = m.footnote_refs.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["2"]); // the def's own [^1] is suppressed, [^2] kept
    }

    #[test]
    fn reserved_ranges_skip_heading_lines() {
        // A dialect owning the whole first line stops the base emitting a
        // heading from it.
        let body = "# Owned\n\n# Real\n";
        let m = scan(body, 0, &[(0, 7)]);
        let texts: Vec<_> = m.headings.iter().map(|h| h.text.as_str()).collect();
        assert_eq!(texts, vec!["Real"]);
    }
}
