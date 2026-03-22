import { useEffect } from 'react'
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext'
import { $getSelection, $isRangeSelection, type LexicalNode } from 'lexical'

/**
 * LivePreviewPlugin implements Obsidian-style "Live Preview" behaviour:
 *
 * - Adds the CSS class `active` to every DOM element that contains the
 *   cursor (the focused node and all its ancestors up to the root).
 * - The CSS for `.active` elements reveals hidden markdown markers:
 *     heading::before  → "# ", "## ", …
 *     .editor-text-bold::before/after → "**"
 *     .editor-text-italic::before/after → "*"
 *     .editor-text-code::before/after → "`"
 *   These pseudo-elements are `display:none` by default and become
 *   `display:inline` when their container has `.active`.
 * - Runs after every editor update so it stays in sync with Lexical's
 *   reconciler even when Lexical re-renders individual nodes.
 */
export function LivePreviewPlugin() {
  const [editor] = useLexicalComposerContext()

  useEffect(() => {
    return editor.registerUpdateListener(({ editorState }) => {
      const root = editor.getRootElement()
      if (!root) return

      // Clear previous active markers
      root.querySelectorAll('.active').forEach(el => el.classList.remove('active'))

      editorState.read(() => {
        const selection = $getSelection()
        if (!$isRangeSelection(selection)) return

        const markedKeys = new Set<string>()

        selection.getNodes().forEach(node => {
          // Walk up the tree: text node → paragraph/heading → … → root
          let current: LexicalNode | null = node
          while (current !== null) {
            const key = current.getKey()
            if (key === 'root') break
            if (!markedKeys.has(key)) {
              markedKeys.add(key)
              const el = editor.getElementByKey(key)
              if (el) el.classList.add('active')
            }
            current = current.getParent()
          }
        })
      })
    })
  }, [editor])

  return null
}
