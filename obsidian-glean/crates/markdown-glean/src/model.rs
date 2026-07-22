//! Data types for the generic Markdown model.
//!
//! The `#[repr(u64)]` enums are serialized to Glean as the 0-based index of the
//! matching `enum` alternative in `schema/markdown.angle`; the discriminants
//! here fix that wire encoding and MUST match the schema ordering.

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
    pub fn end(&self) -> usize {
        self.start + self.length
    }
}

/// Generic frontmatter value type. Matches `type FmType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum FmType {
    Null = 0,
    Boolean = 1,
    Number = 2,
    String = 3,
    Date = 4,
    Datetime = 5,
    Array = 6,
    Object = 7,
}

impl FmType {
    pub fn index(self) -> u64 {
        self as u64
    }
}

/// A single scalar frontmatter value. Matches `type FmValue` (arrays expand to
/// one value per element, so there is no array variant here).
#[derive(Debug, Clone, PartialEq)]
pub enum FmValue {
    Null,
    Boolean(bool),
    Number(String),
    String(String),
    Date(String),
    Datetime(String),
    /// A nested object/array, carried as compact JSON text.
    Object(String),
}

/// A frontmatter property: a name, its top-level type, and its scalar value(s).
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: String,
    pub ftype: FmType,
    pub values: Vec<FmValue>,
}

/// Whether a heading was written ATX (`#`) or Setext (underline). Matches
/// `type HeadingKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum HeadingKind {
    Atx = 0,
    Setext = 1,
}

impl HeadingKind {
    pub fn index(self) -> u64 {
        self as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub text: String,
    pub level: u64,
    pub slug: String,
    pub kind: HeadingKind,
    pub span: Span,
}

/// The syntactic form of a link. Matches `type LinkKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum LinkKind {
    Inline = 0,
    Reference = 1,
    Collapsed = 2,
    Shortcut = 3,
    Autolink = 4,
}

impl LinkKind {
    pub fn index(self) -> u64 {
        self as u64
    }
}

/// A link or image occurrence, before external/internal classification.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub kind: LinkKind,
    pub image: bool,
    /// The destination as written (after reference-label resolution), minus any
    /// surrounding `<>`; may include a `#fragment`.
    pub dest: String,
    /// The link display text (`[text]`), or the URL for a bare autolink.
    pub text: String,
    pub span: Span,
}

/// A link reference definition `[label]: destination`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRefDef {
    pub label: String,
    pub destination: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeBlock {
    /// First info-string word, lower-cased (empty if none).
    pub language: String,
    /// Full info string as written.
    pub info: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlAttr {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlElement {
    pub name: String,
    pub span: Span,
    pub attrs: Vec<HtmlAttr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskItem {
    pub checked: bool,
    /// The character inside the brackets (`" "`, `"x"`, `"X"`, ...).
    pub marker: String,
    pub text: String,
    /// Byte range of the whole item line (used to attribute links to it).
    pub span: Span,
}

/// Column alignment in a GFM table. Matches `type ColumnAlign`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum ColumnAlign {
    None = 0,
    Left = 1,
    Center = 2,
    Right = 3,
}

impl ColumnAlign {
    pub fn index(self) -> u64 {
        self as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableColumn {
    pub align: ColumnAlign,
    pub header: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub columns: Vec<TableColumn>,
    pub rows: u64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footnote {
    pub label: String,
    pub span: Span,
}

/// Everything the generic scanner extracts from one document body (frontmatter
/// is parsed separately).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarkdownContent {
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    pub link_defs: Vec<LinkRefDef>,
    pub code_blocks: Vec<CodeBlock>,
    pub html: Vec<HtmlElement>,
    pub tasks: Vec<TaskItem>,
    pub tables: Vec<Table>,
    pub footnote_defs: Vec<Footnote>,
    pub footnote_refs: Vec<Footnote>,
}
