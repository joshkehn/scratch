# markdown-glean

A Glean indexer for generic Markdown (CommonMark + the GFM superset), and the
extensible base other Markdown dialects build on.

It walks a corpus of Markdown files and emits `markdown.*` Glean facts (schema:
[`schema/markdown.angle`](schema/markdown.angle)) so the corpus becomes a
queryable graph — cross-references, outline, code blocks, external links,
tables, and more.

## Quick start

```sh
cargo run --release -p markdown-glean -- <dir> [-o out.json] [--pretty]
```

Load into Glean:

```sh
glean create --schema schema/markdown.angle --db md/0 out.json --finish
```

## What gets indexed

- **Files & documents** — `File` (vault-relative path), `FileName`,
  `FileExtension`; a Markdown file is also a `Document`. Non-Markdown files are
  interned too, so links can resolve to them.
- **Frontmatter** — a generic, dialect-neutral view of a leading `---` YAML
  block: `FrontmatterPresent`, `FrontmatterKey`, `FrontmatterKeyType` (type is
  `null / boolean / number / string / date / datetime / array / object`), and
  `FrontmatterKeyValue` (one value per array element; nested objects as JSON).
- **Headings** — `Heading { text, level, slug, kind }` for both ATX (`#`) and
  Setext (`===`/`---`) headings, with a GitHub-style `slug` for `#fragment`
  resolution.
- **Links / images / URLs** — every link and image becomes one of:
  - `Reference { source, target: File, kind, image, text, fragment }` — an
    internal link resolved to a corpus file (with the `#fragment` if any);
  - `ExternalLink { source, url, scheme, kind, image, text }` — a link to a URL
    (including `<autolinks>`, bare `https://…` URLs, and `mailto:` emails);
  - `UnresolvedReference` — an internal-looking link that didn't resolve.
  `kind` is `inline / reference / collapsed / shortcut / autolink`; `image` is
  a boolean. `LinkRefDefinition` records `[label]: dest` definitions.
- **Code blocks** — `CodeBlock { language, info }` per fenced block.
- **Inline HTML** — `HtmlElement { name }` + `HtmlAttribute { name, value }`.
- **Task items** — `TaskItem { checked, marker, text }` (GFM `- [ ]`/`- [x]`),
  and `TaskItemLink` for a resolved link inside a task.
- **Tables** — `Table { columns, rows }` + `TableColumn { index, align, header }`
  (GFM pipe tables).
- **Footnotes** — `FootnoteDefinition` / `FootnoteReference`, paired by the
  derived `FootnoteResolved`.
- **Backlinks** — the derived `Backlink` (reverse of `Reference`).

## Example queries

```angle
# Every document that links to CONTRIBUTING.md
S where markdown.Reference { source = S, target = markdown.File "CONTRIBUTING.md" }

# All external URLs a doc links to
markdown.ExternalLink { source = D, url = U } where
  markdown.Document D = markdown.Document (markdown.File "README.md")

# Documents with a Rust code block
D where markdown.CodeBlock { doc = D, language = "rust" }

# Documents that declare a frontmatter `draft` of type boolean
D where markdown.FrontmatterKeyType { doc = D, key = "draft", type_ = boolean }

# Wide tables (5+ columns)
markdown.Table { doc = D, columns = N } where N > 4

# Backlinks to a file
markdown.Backlink { target = markdown.File "docs/guide.md", source = S }
```

## Extending it

Implement `markdown_glean::Dialect` in your own crate to add syntax and facts
(see the top-level README and the `obsidian-glean` crate). Your dialect shares
the base fact-id space, reserves its own syntax spans from the generic scanner,
and provides link resolution used for base links too.

## Limitations

Parsing is a pragmatic regex/line scanner, not a full CommonMark parser:

- Nested image-in-link (`[![alt](img)](href)` badges) is approximated — both
  URLs are captured but the `image` flag/kind may be imperfect.
- Indented (4-space) code blocks and fenced blocks nested inside deeply
  indented list items are not detected as code.
- Unquoted YAML numbers are parsed (so `1.20` normalizes to `1.2`); quote
  version-like values to preserve the exact lexeme.
- Reference-label normalization is ASCII case-folding.

## Testing

```sh
cargo test -p markdown-glean
```
