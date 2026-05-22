import { useState, useRef, useEffect } from 'react'
import { ModalShell, FieldLabel, ModalActions } from './ModalShell'

export const TAG_COLORS = ['#bf8b5e', '#e07a5f', '#f2cc8f', '#81b29a', '#6a9fb5', '#a78bca', '#e8a0b4', '#7fb5b5']
export const TAG_COLOR_NAMES = ['Tan', 'Red', 'Yellow', 'Green', 'Blue', 'Purple', 'Pink', 'Teal']

export function AddTagModal({ onAdd, onClose }: {
  onAdd: (name: string, color: string) => void
  onClose: () => void
}) {
  const [name, setName] = useState('')
  const [color, setColor] = useState(TAG_COLORS[0])
  const ref = useRef<HTMLInputElement>(null)

  useEffect(() => { ref.current?.focus() }, [])

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    onAdd(name.trim(), color)
  }

  return (
    <ModalShell title="New Tag" onClose={onClose}>
      <form onSubmit={handleSubmit} className="p-6 flex flex-col gap-5">
        <div>
          <FieldLabel htmlFor="tag-name">Name</FieldLabel>
          <input id="tag-name" ref={ref} value={name} onChange={(e) => setName(e.target.value)} placeholder="Tag name" className="ff" />
        </div>
        <div>
          <FieldLabel>Color</FieldLabel>
          <div className="flex gap-2.5 pt-1" role="radiogroup" aria-label="Tag color">
            {TAG_COLORS.map((c, i) => (
              <button
                key={c}
                type="button"
                onClick={() => setColor(c)}
                aria-label={TAG_COLOR_NAMES[i]}
                aria-pressed={color === c}
                className="w-6 h-6 rounded-full transition-transform duration-100 relative cursor-pointer"
                style={{ background: c, transform: color === c ? 'scale(1.2)' : 'scale(1)' }}
              >
                {color === c && (
                  <span className="absolute inset-0 flex items-center justify-center text-[#0c0b0a]" aria-hidden="true">
                    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={3} strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>
        <ModalActions onClose={onClose} submitLabel="Create tag" />
      </form>
    </ModalShell>
  )
}
