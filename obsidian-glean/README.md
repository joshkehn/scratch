# markdown-glean / obsidian-glean

Glean indexers for Markdown, structured as a reusable **base + dialect
extension**:

- **[`markdown-glean`](crates/markdown-glean)** — a dialect-neutral indexer for
  generic Markdown (CommonMark + the GFM superset). Point it at any Markdown
  corpus (a repo's `README.md`, `CONTRIBUTING.md`, `docs/`, a blog, …) and it
  emits `markdown.*` Glean facts: files, documents, frontmatter, headings,
  links / images / external URLs, code blocks, inline HTML, task items, GFM
  tables and footnotes.
- **[`obsidian-glean`](crates/obsidian-glean)** — an Obsidian *dialect* that
  **extends** the base. It adds `obsidian.*` facts (wikilinks/embeds, tags with
  a nesting hierarchy, block ids, note titles/aliases, Obsidian-typed
  properties) and emits them **alongside** the base facts, so one indexer over
  a vault produces both `markdown.*` and `obsidian.*`.

The point of the split: run **one** indexer on an Obsidian vault and still get
the generic Markdown graph, run the base indexer on a plain repo, and let a
third party add their own dialect (MyST, Hugo, Quarto, …) by implementing one
trait against `markdown-glean` — without forking the parser.

## Layout

```
Cargo.toml                       workspace
crates/
  markdown-glean/                base library + `markdown-glean-indexer` binary
    schema/markdown.angle
    src/{scan,table,frontmatter,emit,facts,resolve,corpus,path,model}.rs
  obsidian-glean/                extension library + `obsidian-glean-indexer` binary
    schema/obsidian.angle        (imports markdown.1)
    src/{scan,dialect,model}.rs
examples/
  markdown-docs/                 a plain-Markdown corpus (README/CONTRIBUTING/docs)
  obsidian-vault/                a small Obsidian vault
```

## Quick start

```sh
cargo build --release

# Plain Markdown corpus -> markdown.* facts
./target/release/markdown-glean-indexer examples/markdown-docs -o docs.json

# Obsidian vault -> markdown.* AND obsidian.* facts
./target/release/obsidian-glean-indexer examples/obsidian-vault -o vault.json
```

Load into Glean (the Obsidian output needs both schemas):

```sh
glean create --schema crates/markdown-glean/schema/markdown.angle \
             --schema crates/obsidian-glean/schema/obsidian.angle \
             --db md/0 vault.json --finish
```

## Extending: writing your own dialect

The base drives generic extraction; a dialect implements
[`markdown_glean::Dialect`](crates/markdown-glean/src/corpus.rs) to add syntax
and facts. It has five hooks, all sharing the base's single fact-id space:

```rust
pub trait Dialect: Resolve {
    type Content;
    fn predicate_order(&self) -> &'static [&'static str];      // your facts' emit order
    fn extra_document_extensions(&self) -> &'static [&'static str] { &[] }
    fn prepare(&mut self, corpus: &Corpus);                     // build cross-doc state
    fn scan(&self, ctx: &DocContext) -> (Self::Content, Vec<(usize, usize)>); // parse + reserve spans
    fn emit(&self, ctx, content, md, base, sink: &mut FactBuilder);           // emit your facts
    fn finish(&self, sink: &mut FactBuilder) {}                 // cross-doc facts
}
```

- **`scan`** returns your parsed constructs plus the byte ranges your syntax
  occupies. The generic scanner treats those as *reserved* and won't re-parse
  them (this is how `[[wikilinks]]` don't get read as base shortcut links).
- **`Resolve`** (a supertrait) is used for *both* your links and the base's
  Markdown links, so your resolution rules apply uniformly.
- **`emit`** writes through the shared `FactBuilder`; `sink.file_id(path)`
  returns the same `File` id the base interned, so your facts can reference base
  entities.

See `crates/obsidian-glean/src/dialect.rs` for a complete ~350-line example.

## Testing

```sh
cargo test --workspace   # unit + end-to-end tests for both crates
cargo clippy --workspace # lint-clean
```

Verified against the real [obsidian-help](https://github.com/obsidianmd/obsidian-help)
vault (307 files): the Obsidian output preserves the prior single-schema
indexer's results (1783 resolved references, 6 tags, 173 aliases, …) while
adding the generic base facts (external links, tables, footnotes).

See each crate's README for its schema and query examples.
