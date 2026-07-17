//! An indexer that turns an Obsidian vault into Glean facts for the
//! `obsidian.notes` schema (see `schema/obsidian.angle`).
//!
//! The entry point is [`index_vault`], which walks a vault directory and
//! returns a `serde_json::Value` holding the Glean JSON fact document.

pub mod facts;
pub mod frontmatter;
pub mod markdown;
pub mod model;
pub mod vault;

pub use vault::{index_vault, Stats};
