//! Corpus walking and the indexing driver + `Dialect` extension trait.

use crate::emit::{self, DocEmit, MARKDOWN_ORDER};
use crate::facts::FactBuilder;
use crate::frontmatter::{parse_properties, split_frontmatter};
use crate::model::{MarkdownContent, Property};
use crate::path::{basename, extension, is_document, normalize_rel};
use crate::resolve::{PathResolver, Resolve};
use crate::scan;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use walkdir::WalkDir;

/// The default document extensions (Markdown).
pub const DEFAULT_DOC_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd", "mdx"];

/// A parsed Markdown document in the corpus.
pub struct DocInfo {
    pub rel: String,
    pub file_id: u64,
    pub doc_id: u64,
    pub properties: Vec<Property>,
    pub frontmatter_present: bool,
    pub content: String,
    pub body_offset: usize,
}

/// The whole corpus, available to a dialect's `prepare` for building indices.
pub struct Corpus {
    /// Every file path (documents and other files), sorted.
    pub files: Vec<String>,
    pub path_set: HashSet<String>,
    pub docs: Vec<DocInfo>,
}

/// Per-document context passed to a dialect's `scan`/`emit`.
pub struct DocContext<'a> {
    pub rel: &'a str,
    pub doc_id: u64,
    pub file_id: u64,
    /// The body after any frontmatter.
    pub body: &'a str,
    /// Byte offset of `body` within the whole file (added to spans).
    pub body_offset: usize,
    pub properties: &'a [Property],
    pub frontmatter_present: bool,
}

/// A Markdown dialect extension. Implement this in a separate crate to add
/// syntax (e.g. Obsidian wikilinks/tags) and emit extra facts. The dialect is
/// also the link [`Resolve`]r, so its resolution applies to base Markdown links
/// as well as its own link forms.
pub trait Dialect: Resolve {
    /// The dialect's per-document parsed constructs.
    type Content;

    /// Predicate emission order for this dialect's facts, appended after the
    /// base `markdown.*` order (referenced facts before their referrers).
    fn predicate_order(&self) -> &'static [&'static str];

    /// Extra file extensions (besides Markdown) that count as documents.
    fn extra_document_extensions(&self) -> &'static [&'static str] {
        &[]
    }

    /// Build cross-document state (name/alias indices, resolver) from the whole
    /// corpus, once, before per-document emission.
    fn prepare(&mut self, corpus: &Corpus);

    /// Scan a document body for dialect syntax. Returns the parsed content and
    /// the byte ranges (body coordinates) the dialect owns, which the generic
    /// scanner will not re-parse.
    fn scan(&self, ctx: &DocContext) -> (Self::Content, Vec<(usize, usize)>);

    /// Emit the dialect's facts for one document. `base` gives access to the
    /// base facts already emitted (e.g. task-item ids for containment).
    fn emit(
        &self,
        ctx: &DocContext,
        content: &Self::Content,
        md: &MarkdownContent,
        base: &DocEmit,
        sink: &mut FactBuilder,
    );

    /// Emit cross-document facts after every document has been visited.
    fn finish(&self, _sink: &mut FactBuilder) {}
}

/// The no-op dialect: plain Markdown with standard path resolution.
#[derive(Default)]
pub struct NoDialect {
    resolver: Option<PathResolver>,
}

impl Resolve for NoDialect {
    fn resolve(&self, target: &str, source_rel: &str) -> Option<String> {
        self.resolver.as_ref()?.resolve(target, source_rel)
    }
}

impl Dialect for NoDialect {
    type Content = ();

    fn predicate_order(&self) -> &'static [&'static str] {
        &[]
    }

    fn prepare(&mut self, corpus: &Corpus) {
        self.resolver = Some(PathResolver::new(corpus.path_set.clone()));
    }

    fn scan(&self, _ctx: &DocContext) -> ((), Vec<(usize, usize)>) {
        ((), Vec::new())
    }

    fn emit(
        &self,
        _ctx: &DocContext,
        _content: &(),
        _md: &MarkdownContent,
        _base: &DocEmit,
        _sink: &mut FactBuilder,
    ) {
    }
}

/// Summary counts from an indexing run.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub files: usize,
    pub documents: usize,
    pub headings: usize,
    pub links: usize,
    pub code_blocks: usize,
    pub tables: usize,
    pub tasks: usize,
}

/// Index a corpus rooted at `root` with the given dialect, returning the Glean
/// JSON fact document and run statistics.
pub fn index_corpus<D: Dialect>(root: &Path, mut dialect: D) -> std::io::Result<(Value, Stats)> {
    let mut doc_exts: Vec<&str> = DEFAULT_DOC_EXTENSIONS.to_vec();
    doc_exts.extend(dialect.extra_document_extensions());

    let mut md_files: Vec<(String, String)> = Vec::new();
    let mut other_files: Vec<String> = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = match entry.path().strip_prefix(root) {
            Ok(p) => normalize_rel(p),
            Err(_) => continue,
        };
        if rel.split('/').any(|c| c.starts_with('.')) {
            continue; // skip dotfiles / .git / .obsidian etc.
        }
        if is_document(&rel, &doc_exts) {
            match std::fs::read_to_string(entry.path()) {
                Ok(content) => md_files.push((rel, content)),
                Err(_) => other_files.push(rel), // non-UTF-8: treat as a plain file
            }
        } else {
            other_files.push(rel);
        }
    }
    md_files.sort_by(|a, b| a.0.cmp(&b.0));
    other_files.sort();

    let mut sink = FactBuilder::new();
    let mut path_set: HashSet<String> = HashSet::new();
    let mut files: Vec<String> = Vec::new();

    // Intern every file (base `File` facts) so links can resolve to any file.
    for (rel, _) in &md_files {
        sink.intern_file(rel, basename(rel), extension(rel).as_deref());
        path_set.insert(rel.clone());
        files.push(rel.clone());
    }
    for rel in &other_files {
        sink.intern_file(rel, basename(rel), extension(rel).as_deref());
        path_set.insert(rel.clone());
        files.push(rel.clone());
    }
    files.sort();

    // Intern documents and parse frontmatter.
    let mut docs: Vec<DocInfo> = Vec::with_capacity(md_files.len());
    for (rel, content) in md_files {
        let file_id = sink.file_id(&rel).expect("file interned above");
        let doc_id = emit::emit_document(&mut sink, file_id);
        let (yaml, _body, body_offset) = split_frontmatter(&content);
        let properties = yaml.map(parse_properties).unwrap_or_default();
        docs.push(DocInfo {
            rel,
            file_id,
            doc_id,
            properties,
            frontmatter_present: yaml.is_some(),
            content,
            body_offset,
        });
    }

    let corpus = Corpus {
        files,
        path_set,
        docs,
    };
    dialect.prepare(&corpus);

    let mut stats = Stats {
        files: corpus.files.len(),
        documents: corpus.docs.len(),
        ..Stats::default()
    };

    for doc in &corpus.docs {
        emit::emit_frontmatter(
            &mut sink,
            doc.doc_id,
            doc.frontmatter_present,
            &doc.properties,
        );

        let body = &doc.content[doc.body_offset..];
        let ctx = DocContext {
            rel: &doc.rel,
            doc_id: doc.doc_id,
            file_id: doc.file_id,
            body,
            body_offset: doc.body_offset,
            properties: &doc.properties,
            frontmatter_present: doc.frontmatter_present,
        };

        let (dialect_content, reserved) = dialect.scan(&ctx);
        let md = scan::scan(body, doc.body_offset, &reserved);

        let base = emit::emit_content(&mut sink, doc.doc_id, &doc.rel, &md, &dialect);
        dialect.emit(&ctx, &dialect_content, &md, &base, &mut sink);

        stats.headings += md.headings.len();
        stats.links += md.links.len();
        stats.code_blocks += md.code_blocks.len();
        stats.tables += md.tables.len();
        stats.tasks += md.tasks.len();
    }

    dialect.finish(&mut sink);

    let mut order: Vec<&'static str> = MARKDOWN_ORDER.to_vec();
    order.extend(dialect.predicate_order());
    Ok((sink.finish(&order), stats))
}
