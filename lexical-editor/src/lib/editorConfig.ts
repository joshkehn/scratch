import { HeadingNode, QuoteNode } from '@lexical/rich-text'
import { CodeNode, CodeHighlightNode } from '@lexical/code'
import { ListNode, ListItemNode } from '@lexical/list'
import { LinkNode, AutoLinkNode } from '@lexical/link'
import { HorizontalRuleNode } from '@lexical/react/LexicalHorizontalRuleNode'
import type { Klass, LexicalNode } from 'lexical'

export const editorTheme = {
  heading: {
    h1: 'editor-h1',
    h2: 'editor-h2',
    h3: 'editor-h3',
    h4: 'editor-h4',
    h5: 'editor-h5',
    h6: 'editor-h6',
  },
  text: {
    bold: 'editor-text-bold',
    italic: 'editor-text-italic',
    code: 'editor-text-code',
    strikethrough: 'editor-text-strikethrough',
    underline: 'editor-text-underline',
  },
  paragraph: 'editor-paragraph',
  quote: 'editor-quote',
  code: 'editor-code',
  codeHighlight: {
    atrule: 'editor-token-attr',
    attr: 'editor-token-attr',
    boolean: 'editor-token-property',
    builtin: 'editor-token-selector',
    cdata: 'editor-token-comment',
    char: 'editor-token-selector',
    class: 'editor-token-function',
    'class-name': 'editor-token-function',
    comment: 'editor-token-comment',
    constant: 'editor-token-property',
    deleted: 'editor-token-property',
    doctype: 'editor-token-comment',
    entity: 'editor-token-operator',
    function: 'editor-token-function',
    important: 'editor-token-variable',
    inserted: 'editor-token-selector',
    keyword: 'editor-token-attr',
    namespace: 'editor-token-variable',
    number: 'editor-token-property',
    operator: 'editor-token-operator',
    prolog: 'editor-token-comment',
    property: 'editor-token-property',
    punctuation: 'editor-token-punctuation',
    regex: 'editor-token-variable',
    selector: 'editor-token-selector',
    string: 'editor-token-selector',
    symbol: 'editor-token-property',
    tag: 'editor-token-property',
    url: 'editor-token-operator',
    variable: 'editor-token-variable',
  },
  list: {
    ol: 'editor-list-ol',
    ul: 'editor-list-ul',
    listitem: 'editor-listitem',
    listitemChecked: 'editor-listitem-checked',
    listitemUnchecked: 'editor-listitem-unchecked',
    nested: {
      listitem: 'editor-nested-listitem',
    },
  },
  link: 'editor-link',
}

export const editorNodes: Array<Klass<LexicalNode>> = [
  HeadingNode,
  QuoteNode,
  CodeNode,
  CodeHighlightNode,
  ListNode,
  ListItemNode,
  LinkNode,
  AutoLinkNode,
  HorizontalRuleNode,
]
