# obsidian-glean

A Glean indexer for [Obsidian](https://obsidian.md) vaults, built as a
[`Dialect`](../markdown-glean) extension of `markdown-glean`.

Indexing a vault emits **both** the generic `markdown.*` facts (headings,
links, code blocks, HTML, tables, frontmatter, …) **and** the Obsidian
`obsidian.*` facts below — into one shared fact-id space, so an
`obsidian.WikiReference` and a `markdown.Reference` both point at the same
`markdown.File`.

## Quick start

```sh
cargo run --release -p obsidian-glean -- <vault-dir> [-o out.json] [--pretty]
```

Load into Glean (both schemas — Obsidian `import`s the base):

```sh
glean create --schema ../markdown-glean/schema/markdown.angle \
             --schema schema/obsidian.angle --db vault/0 out.json --finish
```

## What the Obsidian dialect adds

Schema: [`schema/obsidian.angle`](schema/obsidian.angle) (imports `markdown.1`).

- **Notes** — `Note` (a `markdown.Document`), `NoteTitle { title, absolute }`
  (both `Me` and `About/Me`, the two forms a wikilink may use), and first-class
  `NoteAlias` from the `aliases` property.
- **Wikilinks & embeds** — `WikiReference { source, target: markdown.File,
  kind, anchor, alias }` for `[[note]]` / `![[embed]]`, resolved by note name /
  alias / path (not just relative path). `anchor` is whole-file / `heading` /
  `block`. Dangling links become `UnresolvedWikiReference`. `LinkAlias { target,
  alias }` records observed aliases (even for non-existent targets) for alias
  suggestions.
- **Tags** — `Tag` (lower-cased), `NoteTag`, and `TagParent` for the nesting
  hierarchy (`#2025/12/20` → `2025/12` → `2025`). `TaskItemTag` attaches a tag
  inside a task item to the base `markdown.TaskItem`.
- **Blocks** — `Block { note, id }` for `^block-id` link targets.
- **Typed properties** — `NotePropertyType { note, key, type_ }` narrows the
  base generic type into Obsidian's `text / list / number / checkbox / date /
  datetime`.
- **Backlinks** — the derived `obsidian.Backlink` (reverse of `WikiReference`);
  combine with `markdown.Backlink` for Markdown-style links.

## Example queries

```angle
# Notes with a given tag
N where obsidian.NoteTag { note = N, tag = "project/active" }

# Wikilink backlinks to Home.md
S where obsidian.Backlink { target = markdown.File "Home.md", source = S }

# Alias suggestions: display names used when linking to "Properties"
A where obsidian.LinkAlias { target = "Properties", alias = A }

# Child tags one level under 2025/12 (distinguishes #2025/12/20 from #2025/1200)
T where obsidian.TagParent { tag = T, parent = "2025/12" }

# Notes with a date-typed property
N where obsidian.NotePropertyType { note = N, type_ = date }

# A wikilink's resolved target and heading anchor
obsidian.WikiReference { source = N, target = T, anchor = { heading = H } }
```

Because the base facts are emitted too, generic queries work on a vault as
well — e.g. `markdown.CodeBlock { language = "php" }`,
`markdown.ExternalLink { scheme = "https" }`, or `markdown.Heading`.

## Relationship to the base

This crate is a thin extension: `src/scan.rs` extracts Obsidian syntax and
reserves its `[[...]]` spans from the base scanner; `src/dialect.rs` implements
the `Dialect` trait and emits the `obsidian.*` facts. Everything generic —
headings, code, HTML, tables, footnotes, Markdown links — comes from the base.

## Testing

```sh
cargo test -p obsidian-glean
```
