import { useState } from 'react'
import { IconPlus } from './icons'

export function EmptyState({ onAdd }: { onAdd: () => void }) {
  const [btnHovered, setBtnHovered] = useState(false)

  return (
    <div className="anim-fade-in flex flex-col items-center justify-center h-full gap-5 text-center px-8">
      <div
        className="w-16 h-16 rounded-2xl flex items-center justify-center"
        style={{ background: 'var(--accent-dim)', border: '1px solid var(--border-mid)' }}
        aria-hidden="true"
      >
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.25} strokeLinecap="round" strokeLinejoin="round" style={{ color: 'var(--accent)' }}>
          <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" />
        </svg>
      </div>
      <div>
        <p className="font-semibold text-base" style={{ color: 'var(--text-primary)' }}>Your library is empty</p>
        <p className="text-sm mt-1.5" style={{ color: 'var(--text-secondary)' }}>
          Add your first bookmark to start building your collection.
        </p>
      </div>
      <button
        onClick={onAdd}
        onMouseEnter={() => setBtnHovered(true)}
        onMouseLeave={() => setBtnHovered(false)}
        className="flex items-center gap-2 px-5 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 cursor-pointer"
        style={{ background: 'var(--accent)', color: '#0c0b0a', opacity: btnHovered ? 0.88 : 1 }}
      >
        <IconPlus size={14} />
        Add Bookmark
      </button>
    </div>
  )
}
