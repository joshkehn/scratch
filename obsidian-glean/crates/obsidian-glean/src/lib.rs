//! A Glean indexer for Obsidian vaults, implemented as a [`Dialect`] extension
//! of the generic `markdown-glean` base.
//!
//! Indexing a vault emits both the base `markdown.*` facts and the Obsidian
//! `obsidian.*` facts (wikilinks/embeds, tags, blocks, note titles/aliases,
//! typed properties) into one shared fact-id space.
//!
//! [`Dialect`]: markdown_glean::Dialect
//!
//! ```no_run
//! use markdown_glean::index_corpus;
//! use obsidian_glean::ObsidianDialect;
//! let (facts, stats) =
//!     index_corpus(std::path::Path::new("vault"), ObsidianDialect::default()).unwrap();
//! ```

pub mod dialect;
pub mod model;
pub mod scan;

pub use dialect::{ObsidianDialect, OBSIDIAN_ORDER};
