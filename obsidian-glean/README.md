# obsidian-glean

A [Glean](https://glean.software) indexer for [Obsidian](https://obsidian.md)
vaults. It walks a vault on disk, parses the Markdown notes the way Obsidian
does (YAML frontmatter, wikilinks, embeds, reference-style links, tags,
headings, block IDs, code blocks, tasks, inline HTML), and emits **Glean facts
as JSON** for the `obsidian.notes` schema.

The point is to turn a vault into a queryable graph so that tooling — for
example a Language Server that lets you jump between notes, list backlinks, or
find every note with a given tag or property — can ask questions like *"what
links here?"* or *"which notes have a `due` date?"* against a real fact
database instead of re-parsing Markdown.

- **Schema:** [`schema/obsidian.angle`](schema/obsidian.angle) (`obsidian.notes.1`)
- **Indexer:** a Rust crate in [`src/`](src) → binary `obsidian-glean-indexer`
- **Example vault:** [`examples/sample-vault/`](examples/sample-vault)

## Quick start

```sh
# Build
cargo build --release

# Index a vault -> Glean JSON on stdout (compact), summary on stderr
./target/release/obsidian-glean-indexer /path/to/vault > facts.json

# Or write to a file, pretty-printed
./target/release/obsidian-glean-indexer /path/to/vault --pretty -o facts.json
```

Load it into a Glean database:

```sh
glean create --schema schema/obsidian.angle --db obsidian/0 facts.json --finish
glean shell --db obsidian/0
```

Try it on the Obsidian help docs (a real vault):

```sh
git clone --depth 1 https://github.com/obsidianmd/obsidian-help
./target/release/obsidian-glean-indexer obsidian-help/en -o help.facts.json
# indexed 307 files (171 notes): 1783 references, 6 unresolved links, ...
```

## What gets indexed

### Files and notes

Every file in the vault becomes a `File` (keyed by its vault-relative path).
Each file also gets a `FileName` (base name) and, when it has one, a
`FileExtension`. This is what makes non-Markdown attachments — images, PDFs,
audio — queryable, and it is what note references resolve *to*.

A `File` keeps the full vault-relative **path** as its key (`"About/Me.md"`),
while `FileName` holds the **name** separately (`"Me.md"`).

A Markdown file (`.md`) additionally becomes a `Note`. Its `NoteTitle` records
two names a wikilink may use: the bare `title` (`"Me"`, for `[[Me]]`) and the
vault-path `absolute` title (`"About/Me"`, for `[[About/Me]]`). Frontmatter
`aliases` are promoted to first-class `NoteAlias` facts (a third way a link can
resolve to the note), and `NoteFrontmatter { note, present }` is emitted for
every note so that "has frontmatter" and "has no frontmatter" are both direct
queries.

### Frontmatter properties → *has key / of type / with value*

Obsidian frontmatter properties are exposed at three levels of specificity:

| Predicate     | Meaning                                    | Request phrasing               |
|---------------|--------------------------------------------|--------------------------------|
| `HasKey`      | note has a property with this name         | *has key* / *has key of name*  |
| `HasKeyType`  | that property's inferred type              | *has key of type*              |
| `HasKeyValue` | that property's value(s)                   | *has key of name with value*   |

Types mirror Obsidian's property types. `PropertyType` is an Angle `enum`,
encoded in JSON as a **0-based index**:

| index | 0 | 1 | 2 | 3 | 4 | 5 |
|-------|------|------|--------|----------|------|----------|
| type  | text | list | number | checkbox | date | datetime |

Values are a sum type tagged by kind (`{ "text": … }`, `{ "number": … }`,
`{ "checkbox": true }`, `{ "date": … }`, `{ "datetime": … }`). Numbers and
dates are kept in textual form so nothing is lost (Angle has no float type). A
list property emits **one `HasKeyValue` fact per element**.

### Tags → *notes with tag*

Tags come from both inline `#tags` in the body and the `tags` frontmatter
property. Each distinct tag is a `Tag`; each occurrence links a note to a tag
via `NoteTag`. Numeric-only tokens (`#1984`) are rejected, per Obsidian's rules.

Tags are **lower-cased**, so `#Tag`, `#TAG` and `#tag` are the same `Tag` fact
("tag"), matching Obsidian's case-insensitivity. Nested tags keep their
slashes, and every ancestor prefix is also interned with a `TagParent` edge to
its immediate parent — so `#2025/12/20` yields tags `2025/12/20`, `2025/12` and
`2025`, with `2025/12/20 → 2025/12 → 2025`. That lets a query distinguish
`#2025/12/20` (parent `2025/12`) from `#2025/1200` (parent `2025`).

### Links → *references of a note*, *back links*, and alias suggestions

Every internal link in a note body becomes a `Reference` from the source
`Note` to the resolved target `File`:

- **wikilinks** `[[Note]]`, `[[Note|alias]]`, `[[folder/Note]]`, `[[Note.md]]`
- **embeds** `![[Note]]`, `![[image.png]]`, `![[image.png|200]]` (table-escaped `\|` handled)
- **markdown links** `[text](Note.md)`, `[text](path/Note%20name.md)` (percent-decoded)
- **reference-style links** `[text][ref]`, `[ref][]`, `[ref]` with a `[ref]: dest`
  definition elsewhere in the note (footnotes `[^1]` are not links)
- **anchors** `[[Note#Heading]]` and `[[Note#^block-id]]`

Each `Reference` carries a `ByteSpan` locating the link in the source file, an
`Anchor` (`none` / `heading` / `block`), and — for `[[Note|alias]]` — the
`alias` (a `maybe string`, omitted when absent). Links that don't resolve to a
file become an `UnresolvedReference` (dangling links) instead of being dropped.

Every aliased link also emits `LinkAlias { target, alias }`, keyed by the raw
target text so it works even for notes that don't exist yet
(`[[Does not exist|missing]]`). This answers *"what display names do people use
when linking to this note?"* — the candidate aliases to add to its frontmatter,
minus those already in `NoteAlias`.

**Backlinks** are the reverse of `Reference`, provided by the on-demand derived
predicate `Backlink { target, source }` — no `glean derive` step required.

Headings (`Heading`) and block IDs (`Block`) are emitted with their own spans,
so an anchor like `#Heading` or `#^block-id` can be resolved to a jump target.

### Code blocks, tasks, and inline HTML

Other common note content is indexed too:

- **`CodeBlock { note, language, span }`** — one per fenced block; `language`
  is the info-string language lower-cased (`""` when none). Find notes with a
  PHP block: `CodeBlock { language = "php" }`.
- **`Todo { note, checked, text, span }`** — task-list items (`- [ ]` /
  `- [x]`); `checked` is false only for a blank `[ ]`. Notes and tags mentioned
  inside a task are attributed to it via **`TodoLink { todo, target }`** and
  **`TodoTag { todo, tag }`**, so you can ask "unchecked tasks that reference
  note X" or "tasks tagged #urgent".
- **`HtmlElement { note, name, span }`** + **`HtmlAttribute { element, name,
  value }`** — inline HTML (`name`/attribute names lower-cased). Find notes with
  any element that sets a style: filter `HtmlAttribute { name = "style" }`.

## Example Angle queries

These map directly onto the capabilities above (predicate names are under the
`obsidian.notes.` namespace):

```angle
# Notes with a given tag
N where obsidian.notes.NoteTag { note = N, tag = "project/active" }

# References *out of* a note (by title)
R where
  obsidian.notes.NoteTitle { note = N, title = "Internal links" };
  R = obsidian.notes.Reference { source = N }

# Back links: notes that link to Home.md
S where obsidian.notes.Backlink { target = obsidian.notes.File "Home.md", source = S }

# Notes that contain any outgoing reference
N where obsidian.notes.Reference { source = N }

# has key: notes with an "aliases" property
obsidian.notes.HasKey { key = "aliases" }

# has key of type: notes that have any date-typed property
obsidian.notes.HasKeyType { type_ = date }

# has key of name with value
obsidian.notes.HasKeyValue { key = "permalink", value = { text = "/" } }

# Every PNG attachment in the vault
obsidian.notes.FileExtension { extension = "png" }

# Notes that have NO frontmatter
N where obsidian.notes.NoteFrontmatter { note = N, present = false }

# Alias suggestions: display names people used when linking to "Properties"
A where obsidian.notes.LinkAlias { target = "Properties", alias = A }

# Child tags one level under 2025/12 (matches #2025/12/20, not #2025/1200)
T where obsidian.notes.TagParent { tag = T, parent = "2025/12" }

# Resolve a note written either way: [[Me]] or [[About/Me]]
N where obsidian.notes.NoteTitle { note = N, absolute = "About/Me" }

# Notes containing a PHP code block
N where obsidian.notes.CodeBlock { note = N, language = "php" }

# Unchecked tasks that reference another note
T where
  obsidian.notes.Todo { checked = false } = T;
  obsidian.notes.TodoLink { todo = T }

# Notes with any HTML element that sets a style property
N where
  obsidian.notes.HtmlElement { note = N } = E;
  obsidian.notes.HtmlAttribute { element = E, name = "style" }
```

## How this supports LSP "jump between notes"

An LSP server backed by this data has everything it needs:

- **Go to definition** on a link: a `Reference`'s `span` is the clickable
  region in the source file; its `target` File (plus `anchor`) is where to
  jump. For an anchor, look up the matching `Heading`/`Block` span in the
  target note.
- **Find references / backlinks** for a note: query `Backlink` (or
  `Reference { target = … }`).
- **Document symbols / outline:** `Heading` facts with levels and spans.
- **Workspace symbols / completion:** `NoteTitle`, `Tag`, `HasKey`.

Spans are byte offsets into the file contents; the server converts them to
line/column using the document text.

## Obsidian-flavored Markdown coverage

Parsing follows Obsidian's rules for
[internal links](https://obsidian.md/help), tags and properties:

- Frontmatter is the leading `---` … `---` YAML block; properties are typed as
  Text / List / Number / Checkbox / Date / Date & time.
- Links, tags, HTML and task items inside **fenced code blocks and inline code
  are ignored**, including code fences nested inside callouts (`> ```…`).
- Link resolution tries, in order: same-note anchor, source-relative path,
  vault-root path, note name (case-insensitive fallback), then frontmatter
  `aliases`.
- Inline HTML is matched with a pragmatic tag scanner (not a full HTML parser);
  scheme/email autolinks (`<https://…>`, `<a@b>`) are not treated as elements.

**Out of scope (for now):** treating frontmatter values as link properties,
Dataview/Bases queries, footnotes, and full CommonMark edge cases. Link
resolution uses a "nearest name" heuristic rather than Obsidian's exact index;
ambiguous bare names resolve to the shortest path. A task item's links/tags are
attributed by line, so a task that wraps across lines only captures its first
line.

## Project layout

```
schema/obsidian.angle      the obsidian.notes.1 Angle schema
src/
  main.rs                  CLI
  vault.rs                 walk, resolve links, drive fact emission
  frontmatter.rs           YAML frontmatter split + property typing
  markdown.rs              links / tags / headings / blocks / code / todos / html
  facts.rs                 Glean JSON fact builder (id interning)
  model.rs                 shared types (enum indices kept in sync with schema)
tests/index_sample.rs      end-to-end test over examples/sample-vault
examples/sample-vault/     a small vault exercising every feature
```

## Testing

```sh
cargo test        # 27 unit tests + 18 end-to-end tests
cargo clippy      # lint-clean
```

The unit tests cover frontmatter typing and the Markdown extractor (aliases,
escaped pipes, autolinks, callout code fences, tag validity, span accuracy,
reference-style links, footnote exclusion, code-block languages, task items,
HTML attributes). The integration tests index `examples/sample-vault` and
assert on the emitted facts: property types, resolved/unresolved references,
anchors, link/reference aliases, first-class note aliases, frontmatter
presence, lower-cased and nested tags, backlinks, code blocks, task links/tags,
HTML elements/attributes, and that code-masked tokens never leak.

## Notes on the JSON format

Output is a Glean JSON array of `{ "predicate": …, "facts": […] }` blocks.
Referenced entities (`File`, `Note`, `Tag`) are interned once with stable fact
`id`s; other facts reference them with `{ "id": N }`. Blocks are ordered so a
fact is always defined before it is referenced. This is the format accepted by
`glean create … <file>`.
