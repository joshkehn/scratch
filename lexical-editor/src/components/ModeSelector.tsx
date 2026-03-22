type Mode = 'source' | 'rich' | 'reader'

interface Props {
  mode: Mode
  onChange: (mode: Mode) => void
}

const MODES: { value: Mode; label: string; title: string }[] = [
  { value: 'source', label: 'Source', title: 'Raw markdown always visible' },
  { value: 'rich', label: 'Live Preview', title: 'Markdown shown while editing, rendered otherwise' },
  { value: 'reader', label: 'Reader', title: 'Fully rendered, no editing' },
]

export default function ModeSelector({ mode, onChange }: Props) {
  return (
    <div className="mode-selector" role="tablist" aria-label="Editor mode">
      {MODES.map(({ value, label, title }) => (
        <button
          key={value}
          role="tab"
          aria-selected={mode === value}
          className={`mode-btn${mode === value ? ' active' : ''}`}
          onClick={() => onChange(value)}
          title={title}
        >
          {label}
        </button>
      ))}
    </div>
  )
}
