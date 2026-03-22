import { useEffect } from 'react'
import { LexicalComposer } from '@lexical/react/LexicalComposer'
import { RichTextPlugin } from '@lexical/react/LexicalRichTextPlugin'
import { ContentEditable } from '@lexical/react/LexicalContentEditable'
import { HistoryPlugin } from '@lexical/react/LexicalHistoryPlugin'
import { LexicalErrorBoundary } from '@lexical/react/LexicalErrorBoundary'
import { MarkdownShortcutPlugin } from '@lexical/react/LexicalMarkdownShortcutPlugin'
import { HorizontalRulePlugin } from '@lexical/react/LexicalHorizontalRulePlugin'
import { OnChangePlugin } from '@lexical/react/LexicalOnChangePlugin'
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext'
import {
  $convertFromMarkdownString,
  $convertToMarkdownString,
  TRANSFORMERS,
} from '@lexical/markdown'
import type { EditorState } from 'lexical'
import { editorNodes, editorTheme } from '../lib/editorConfig'
import { LivePreviewPlugin } from '../plugins/LivePreviewPlugin'

/**
 * Imports markdown into the editor on mount only.
 * The empty dep array is intentional: we only initialise once.
 * Subsequent mode switches re-mount the whole component (via `key`),
 * so this effect always sees the latest markdown.
 */
function InitMarkdownPlugin({ markdown }: { markdown: string }) {
  const [editor] = useLexicalComposerContext()
  useEffect(() => {
    editor.update(() => {
      $convertFromMarkdownString(markdown, TRANSFORMERS)
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return null
}

interface Props {
  markdown: string
  onChange: (markdown: string) => void
}

/**
 * Rich edit / Live Preview mode.
 *
 * Architecture:
 * - Lexical with RichTextPlugin + MarkdownShortcutPlugin for full markdown
 *   shortcut support (type "# " → heading, "**text**" → bold, etc.).
 * - InitMarkdownPlugin hydrates the editor from the shared markdown state
 *   on every mount (mode switches force a re-mount via `key` in App).
 * - OnChangePlugin keeps the shared markdown state up-to-date so other
 *   modes always see the latest edits.
 * - LivePreviewPlugin adds the CSS class `active` to the DOM element(s)
 *   containing the cursor, which causes the CSS to reveal the markdown
 *   syntax markers (# for headings, ** for bold, etc.) for that element.
 */
export default function RichEditor({ markdown, onChange }: Props) {
  const handleChange = (editorState: EditorState) => {
    editorState.read(() => {
      const md = $convertToMarkdownString(TRANSFORMERS)
      onChange(md)
    })
  }

  return (
    <LexicalComposer
      initialConfig={{
        namespace: 'rich-editor',
        theme: editorTheme,
        nodes: editorNodes,
        editable: true,
        onError: console.error,
      }}
    >
      <div className="rich-editor-wrapper">
        <div className="rich-editor">
          <RichTextPlugin
            contentEditable={
              <ContentEditable
                className="rich-editor-content"
                aria-label="Rich text editor"
              />
            }
            placeholder={
              <div className="editor-placeholder">Start writing…</div>
            }
            ErrorBoundary={LexicalErrorBoundary}
          />
        </div>
      </div>
      <HistoryPlugin />
      <HorizontalRulePlugin />
      <MarkdownShortcutPlugin transformers={TRANSFORMERS} />
      <OnChangePlugin onChange={handleChange} />
      <InitMarkdownPlugin markdown={markdown} />
      <LivePreviewPlugin />
    </LexicalComposer>
  )
}
