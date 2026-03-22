import { useState, useCallback, useRef } from 'react'
import ModeSelector from './components/ModeSelector'
import SourceEditor from './components/SourceEditor'
import RichEditor from './components/RichEditor'
import ReaderView from './components/ReaderView'

type Mode = 'source' | 'rich' | 'reader'

const INITIAL_MARKDOWN = `# Welcome to the Lexical Markdown Editor

This editor follows Obsidian's three-mode approach.

## Modes

Switch between modes using the buttons above:

- **Source** — always shows raw markdown
- **Live Preview** — renders markdown; reveals syntax only for the element your cursor is in
- **Reader** — fully rendered, no editing

## Formatting Examples

This is **bold text**, this is *italic text*, and this is \`inline code\`.

Here is ~~strikethrough~~ text.

> This is a blockquote with _nested italic_ inside.

### Code Block

\`\`\`javascript
function greet(name) {
  return \`Hello, \${name}!\`
}
\`\`\`

### Lists

1. First ordered item
2. Second ordered item
3. Third ordered item

- Unordered item A
- Unordered item B
  - Nested item

### Links

Here is a [link to example.com](https://example.com).

---

Try clicking into a **bold** word or a heading in Live Preview mode — the markdown markers will appear.
`

export default function App() {
  const [mode, setMode] = useState<Mode>('rich')
  const [markdown, setMarkdown] = useState(INITIAL_MARKDOWN)

  // Mount keys: incrementing forces Lexical to re-mount and re-import markdown.
  // We increment when switching TO a Lexical mode so it picks up any edits
  // made in source mode (or the other Lexical mode).
  const richKey = useRef(0)
  const readerKey = useRef(0)

  const handleModeChange = useCallback((newMode: Mode) => {
    if (newMode === 'rich') richKey.current += 1
    if (newMode === 'reader') readerKey.current += 1
    setMode(newMode)
  }, [])

  return (
    <div className="app">
      <header className="app-header">
        <span className="app-title">Lexical Markdown Editor</span>
        <ModeSelector mode={mode} onChange={handleModeChange} />
      </header>
      <main className="app-content">
        {mode === 'source' && (
          <SourceEditor markdown={markdown} onChange={setMarkdown} />
        )}
        {mode === 'rich' && (
          <RichEditor
            key={richKey.current}
            markdown={markdown}
            onChange={setMarkdown}
          />
        )}
        {mode === 'reader' && (
          <ReaderView key={readerKey.current} markdown={markdown} />
        )}
      </main>
    </div>
  )
}
