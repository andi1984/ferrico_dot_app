import { useEffect } from 'react'
import { IconArrowLeft, IconChevronRight } from './icons'

export interface BreadcrumbItem {
  label: string
  onClick?: () => void
}

export function Breadcrumb({ items }: { items: BreadcrumbItem[] }) {
  return (
    <nav aria-label="Settings breadcrumb" className="flex items-center gap-1.5 min-w-0">
      {items.map((item, i) => {
        const isLast = i === items.length - 1
        return (
          <span key={i} className="flex items-center gap-1.5 min-w-0">
            {i > 0 && <IconChevronRight size={11} />}
            {item.onClick && !isLast ? (
              <button
                onClick={item.onClick}
                className="cursor-pointer truncate hover:underline"
                style={{ fontSize: 18, fontWeight: 600, fontFamily: 'var(--font-display)', letterSpacing: '-0.015em', color: 'var(--text-2)' }}
              >
                {item.label}
              </button>
            ) : (
              <span
                className="truncate"
                style={{
                  fontSize: 18,
                  fontWeight: 600,
                  fontFamily: 'var(--font-display)',
                  letterSpacing: '-0.015em',
                  color: isLast ? 'var(--text-1)' : 'var(--text-2)',
                }}
              >
                {item.label}
              </span>
            )}
          </span>
        )
      })}
    </nav>
  )
}

export function SettingsLayout({ breadcrumb, onBack, children }: {
  breadcrumb: BreadcrumbItem[]
  onBack: () => void
  children: React.ReactNode
}) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onBack()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onBack])

  return (
    <div className="fixed inset-0 z-50 flex flex-col anim-fade-in" style={{ background: 'var(--bg)' }}>
      <header
        className="flex items-center gap-3 px-5 py-3.5 flex-none"
        style={{ borderBottom: '1px solid var(--border-soft)', background: 'var(--header-bg)' }}
      >
        <button
          onClick={onBack}
          className="flex items-center justify-center rounded-md transition-colors duration-150 cursor-pointer flex-none"
          style={{ width: 28, height: 28, color: 'var(--text-2)' }}
          onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--btn-hover-bg)'; e.currentTarget.style.color = 'var(--text-1)' }}
          onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-2)' }}
          aria-label="Back"
        >
          <IconArrowLeft size={16} />
        </button>
        <Breadcrumb items={breadcrumb} />
      </header>

      <div className="flex-1 overflow-y-auto">
        <div className="max-w-2xl mx-auto p-6 flex flex-col gap-6">
          {children}
        </div>
      </div>
    </div>
  )
}
