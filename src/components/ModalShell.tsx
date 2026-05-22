import { useEffect } from 'react'
import { IconClose } from './icons'

export function ModalShell({ title, onClose, children }: {
  title: string
  onClose: () => void
  children: React.ReactNode
}) {
  const titleId = `modal-title-${title.replace(/\s+/g, '-').toLowerCase()}`

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      style={{ background: 'rgba(0,0,0,0.75)', backdropFilter: 'blur(4px)' }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose() }}
    >
      <div
        className="anim-scale-in w-full max-w-md rounded-xl shadow-2xl overflow-hidden"
        style={{ background: 'var(--bg-surface)', border: '1px solid var(--border-mid)' }}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <div
          className="flex items-center justify-between px-6 py-4"
          style={{ borderBottom: '1px solid var(--border-dim)' }}
        >
          <span id={titleId} className="text-sm font-semibold tracking-wide" style={{ color: 'var(--text-primary)' }}>
            {title}
          </span>
          <button
            onClick={onClose}
            className="rounded p-1 transition-colors duration-150 cursor-pointer"
            style={{ color: 'var(--text-muted)' }}
            onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-primary)')}
            onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
            aria-label="Close dialog"
          >
            <IconClose size={15} />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}

export function FieldLabel({ children, htmlFor }: { children: React.ReactNode; htmlFor?: string }) {
  return (
    <label
      htmlFor={htmlFor}
      className="block text-xs font-medium uppercase tracking-widest mb-2"
      style={{ color: 'var(--text-muted)' }}
    >
      {children}
    </label>
  )
}

export function ModalActions({ onClose, submitLabel }: { onClose: () => void; submitLabel: string }) {
  return (
    <div className="flex gap-2 pt-2">
      <button
        type="button"
        onClick={onClose}
        className="flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-colors duration-150 cursor-pointer"
        style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
        onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
        onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
      >
        Cancel
      </button>
      <button
        type="submit"
        className="flex-1 px-4 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 hover:opacity-90 cursor-pointer"
        style={{ background: 'var(--accent)', color: '#0c0b0a' }}
      >
        {submitLabel}
      </button>
    </div>
  )
}
