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
      className="modal-backdrop fixed inset-0 z-50 flex items-center justify-center p-4 anim-fade-in"
      onClick={(e) => { if (e.target === e.currentTarget) onClose() }}
    >
      <div
        className="modal-panel anim-scale-in w-full max-w-md rounded-xl shadow-2xl overflow-hidden"
        style={{ border: '1px solid var(--border)' }}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <div
          className="flex items-center justify-between px-5 py-3.5"
          style={{ borderBottom: '1px solid var(--border-soft)' }}
        >
          <span id={titleId} className="section-label" style={{ color: 'var(--text-2)' }}>
            {title}
          </span>
          <button
            onClick={onClose}
            className="flex items-center justify-center rounded-md transition-colors duration-150 cursor-pointer"
            style={{ width: 26, height: 26, color: 'var(--text-2)' }}
            onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--btn-hover-bg)'; e.currentTarget.style.color = 'var(--text-1)' }}
            onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-2)' }}
            aria-label="Close dialog"
          >
            <IconClose size={14} />
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
        className="flex-1 rounded-lg transition-colors duration-150 cursor-pointer"
        style={{
          height: 34,
          background: 'var(--input-bg)',
          border: '1px solid var(--border-soft)',
          color: 'var(--text-1)',
          fontSize: 12.5,
          fontWeight: 500,
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--btn-hover-bg)')}
        onMouseLeave={(e) => (e.currentTarget.style.background = 'var(--input-bg)')}
      >
        Cancel
      </button>
      <button
        type="submit"
        className="btn-accent flex-1 rounded-lg cursor-pointer"
        style={{ height: 34, fontSize: 12.5 }}
      >
        {submitLabel}
      </button>
    </div>
  )
}
