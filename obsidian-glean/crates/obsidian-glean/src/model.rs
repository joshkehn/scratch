//! Obsidian dialect model types. Enum discriminants are the JSON wire format
//! and must match `schema/obsidian.angle`.

use markdown_glean::model::Span;

/// How an Obsidian link was written. Matches `type WikiKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum WikiKind {
    Wikilink = 0,
    Embed = 1,
}

impl WikiKind {
    pub fn index(self) -> u64 {
        self as u64
    }
}

/// A within-file anchor. Matches `type Anchor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    None,
    Heading(String),
    Block(String),
}

/// The Obsidian UI property type. Matches `type PropertyType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum PropertyType {
    Text = 0,
    List = 1,
    Number = 2,
    Checkbox = 3,
    Date = 4,
    Datetime = 5,
}

impl PropertyType {
    pub fn index(self) -> u64 {
        self as u64
    }
}

/// A wikilink or embed occurrence, before resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct WikiLink {
    pub kind: WikiKind,
    /// The link destination as written, minus alias and anchor.
    pub target: String,
    pub anchor: Anchor,
    /// The display text when written `[[target|alias]]` (not for embeds).
    pub alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagOccurrence {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockId {
    pub id: String,
    pub span: Span,
}

/// Everything the Obsidian scanner extracts from one note body.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObsidianContent {
    pub wiki_links: Vec<WikiLink>,
    pub tags: Vec<TagOccurrence>,
    pub blocks: Vec<BlockId>,
}
