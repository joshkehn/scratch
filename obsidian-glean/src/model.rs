//! Data types shared across the indexer.
//!
//! The `PropertyType` and `LinkKind` enums are serialized to Glean as the
//! 0-based index of the matching `enum` alternative in `schema/obsidian.angle`.
//! The `#[repr(u64)]` discriminants below fix that wire encoding, so the order
//! here MUST match the order of alternatives in the schema.

/// A byte range within a file's contents: 0-based `start` and byte `length`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub length: usize,
}

impl Span {
    pub fn new(start: usize, length: usize) -> Self {
        Span { start, length }
    }
}

/// Obsidian property type. Matches `type PropertyType = enum { ... }`.
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
    /// The enum index as written to Glean JSON.
    pub fn index(self) -> u64 {
        self as u64
    }
}

/// A single typed frontmatter value. Matches `type PropertyValue = { ... }`.
/// A `list` property is represented as several `PropertyValue`s, one per
/// element, so there is deliberately no `List` variant here.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Text(String),
    Number(String),
    Checkbox(bool),
    Date(String),
    Datetime(String),
}

/// A frontmatter property: a name, an Obsidian type, and zero or more values.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: String,
    pub ptype: PropertyType,
    pub values: Vec<PropertyValue>,
}

/// How a link was written. Matches `type LinkKind = enum { ... }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum LinkKind {
    Wikilink = 0,
    Embed = 1,
    Markdown = 2,
}

impl LinkKind {
    pub fn index(self) -> u64 {
        self as u64
    }
}

/// A within-file anchor named by a link. Matches `type Anchor = { ... }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    None,
    Heading(String),
    Block(String),
}

/// A link occurrence found in a note body, before resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub kind: LinkKind,
    /// The link destination as written, minus any alias and anchor
    /// (e.g. "Projects/Three laws of motion" or "Figure 1.png"). Empty when
    /// the link is a pure same-note anchor such as `[[#Heading]]`.
    pub target: String,
    pub anchor: Anchor,
    pub span: Span,
}

/// A heading occurrence in a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u64,
    pub text: String,
    pub span: Span,
}

/// A block-identifier definition (`^block-id`) in a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockId {
    pub id: String,
    pub span: Span,
}

/// A tag occurrence, with the leading `#` stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagOccurrence {
    pub name: String,
    pub span: Span,
}

/// Everything extracted from a single note body (excludes frontmatter, which
/// is parsed separately into `Property`s and tags).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteContent {
    pub links: Vec<Link>,
    pub headings: Vec<Heading>,
    pub blocks: Vec<BlockId>,
    pub tags: Vec<TagOccurrence>,
}
