//! GFM pipe-table detection.

use crate::model::{ColumnAlign, Span, Table, TableColumn};
use crate::scan::SrcLine;
use regex::Regex;
use std::sync::OnceLock;

fn delimiter_cell_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*:?-+:?\s*$").unwrap())
}

pub struct TableMatch {
    pub table: Table,
    /// Index of the first line after the table.
    pub next_index: usize,
}

/// Try to parse a GFM table whose header row is `lines[i]` and whose delimiter
/// row is `lines[i + 1]`. Returns `None` if this is not a table.
pub fn try_table(lines: &[SrcLine], i: usize, base: usize) -> Option<TableMatch> {
    let header = lines[i].text;
    let delim = lines[i + 1].text;
    if !header.contains('|') || !delim.contains('-') {
        return None;
    }
    let delim_cells = split_row(delim);
    if delim_cells.is_empty() || !delim_cells.iter().all(|c| delimiter_cell_re().is_match(c)) {
        return None;
    }

    let aligns: Vec<ColumnAlign> = delim_cells.iter().map(|c| align_of(c)).collect();
    let header_cells = split_row(header);
    let columns: Vec<TableColumn> = aligns
        .iter()
        .enumerate()
        .map(|(idx, &align)| TableColumn {
            align,
            header: header_cells.get(idx).cloned().unwrap_or_default(),
        })
        .collect();

    // Consume body rows.
    let mut rows = 0u64;
    let mut j = i + 2;
    let mut end = lines[i + 1].end;
    while j < lines.len() {
        let t = lines[j].text.trim();
        if t.is_empty() || !lines[j].text.contains('|') {
            break;
        }
        rows += 1;
        end = lines[j].end;
        j += 1;
    }

    let span = Span::new(base + lines[i].start, end - lines[i].start);
    Some(TableMatch {
        table: Table {
            columns,
            rows,
            span,
        },
        next_index: j,
    })
}

fn align_of(cell: &str) -> ColumnAlign {
    let c = cell.trim();
    let left = c.starts_with(':');
    let right = c.ends_with(':');
    match (left, right) {
        (true, true) => ColumnAlign::Center,
        (true, false) => ColumnAlign::Left,
        (false, true) => ColumnAlign::Right,
        (false, false) => ColumnAlign::None,
    }
}

/// Split a table row into trimmed cells, honouring escaped pipes (`\|`) and
/// optional leading/trailing pipes.
fn split_row(row: &str) -> Vec<String> {
    let row = row.trim();
    let inner = row.strip_prefix('|').unwrap_or(row);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            // Keep the escaped pipe (or the backslash + char) in the cell text.
            if ch != '|' {
                cur.push('\\');
            }
            cur.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '|' {
            cells.push(cur.trim().to_string());
            cur = String::new();
        } else {
            cur.push(ch);
        }
    }
    if escaped {
        cur.push('\\');
    }
    cells.push(cur.trim().to_string());
    cells
}
