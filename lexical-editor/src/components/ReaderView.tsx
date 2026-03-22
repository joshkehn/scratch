import { useEffect } from 'react'
import { LexicalComposer } from '@lexical/react/LexicalComposer'
import { RichTextPlugin } from '@lexical/react/LexicalRichTextPlugin'
import { ContentEditable } from '@lexical/react/LexicalContentEditable'
import { LexicalErrorBoundary } from '@lexical/react/LexicalErrorBoundary'
import { HorizontalRulePlugin } from '@lexical/react/LexicalHorizontalRulePlugin'
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext'
import { $convertFromMarkdownString, TRANSFORMERS } from '@lexical/markdown'
import { editorNodes, editorTheme } from '../lib/editorConfig'

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
}

/**
 * Reader mode: fully rendered, no editing.
 *
 * - `editable: false` in initialConfig makes the entire editor read-only.
 * - The same theme as the rich editor is used so formatting looks identical.
 * - No LivePreviewPlugin — markdown markers are never shown.
 * - Cursor is set to `default` via CSS so it doesn't look like a text field.
 */
export default function ReaderView({ markdown }: Props) {
  return (
    <LexicalComposer
      initialConfig={{
        namespace: 'reader-view',
        theme: editorTheme,
        nodes: editorNodes,
        editable: false,
        onError: console.error,
      }}
    >
      <div className="reader-wrapper">
        <div className="reader-view">
          <RichTextPlugin
            contentEditable={
              <ContentEditable
                className="reader-content"
                aria-label="Document reader"
                aria-readonly="true"
              />
            }
            placeholder={<></>}
            ErrorBoundary={LexicalErrorBoundary}
          />
        </div>
      </div>
      <HorizontalRulePlugin />
      <InitMarkdownPlugin markdown={markdown} />
    </LexicalComposer>
  )
}
