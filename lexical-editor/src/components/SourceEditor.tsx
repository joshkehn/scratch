interface Props {
  markdown: string
  onChange: (markdown: string) => void
}

/**
 * Source mode: always shows raw markdown in a plain text editor.
 * Uses a textarea for simplicity and correctness — Lexical's paragraph
 * model would collapse blank lines, which are meaningful in markdown.
 */
export default function SourceEditor({ markdown, onChange }: Props) {
  return (
    <textarea
      className="source-editor"
      value={markdown}
      onChange={e => onChange(e.target.value)}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
      aria-label="Markdown source"
    />
  )
}
