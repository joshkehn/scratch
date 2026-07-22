//! A Glean indexer for generic Markdown (CommonMark + GFM).
//!
//! Index a corpus of Markdown files into `markdown.*` Glean facts (see
//! `schema/markdown.angle`). The base is dialect-neutral; a [`Dialect`]
//! extension (e.g. the `obsidian-glean` crate) adds syntax and facts while
//! sharing the base fact-id space.
//!
//! ```no_run
//! use markdown_glean::{index_corpus, NoDialect};
//! let (facts, stats) = index_corpus(std::path::Path::new("docs"), NoDialect::default()).unwrap();
//! ```

pub mod corpus;
pub mod emit;
pub mod facts;
pub mod frontmatter;
pub mod model;
pub mod path;
pub mod resolve;
pub mod scan;
pub mod table;

pub use corpus::{
    index_corpus, Corpus, Dialect, DocContext, DocInfo, NoDialect, Stats, DEFAULT_DOC_EXTENSIONS,
};
pub use emit::{DocEmit, MARKDOWN_ORDER};
pub use facts::{fact_ref, FactBuilder};
pub use model::MarkdownContent;
pub use resolve::{PathResolver, Resolve};
