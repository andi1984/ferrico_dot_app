import { useRef, useEffect } from 'react'

export interface CtxItem {
  label: string
  action: () => void
  danger?: boolean
  sep?: true
}

export interface CtxMenuState { x: number; y: number; items: CtxItem[] }

export function ContextMenu({ state, onClose }: { state: CtxMenuState; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    function handler(e: MouseEvent | KeyboardEvent) {
      if (e instanceof KeyboardEvent && e.key !== 'Escape') return
      if (e instanceof MouseEvent && ref.current?.contains(e.target as Node)) return
      onClose()
    }
    document.addEventListener('mousedown', handler)
    document.addEventListener('keydown', handler)
    return () => {
      document.removeEventListener('mousedown', handler)
      document.removeEventListener('keydown', handler)
    }
  }, [onClose])

  const style: React.CSSProperties = {
    left: Math.min(state.x, window.innerWidth - 180),
    top: Math.min(state.y, window.innerHeight - (state.items.length * 34 + 16)),
  }

  return (
    <div ref={ref} className="ctx-menu" style={style} role="menu">
      {state.items.map((item, i) =>
        item.sep ? <div key={i} className="ctx-sep" role="separator" /> : (
          <button
            key={i}
            className={`ctx-item${item.danger ? ' danger' : ''}`}
            role="menuitem"
            onClick={() => { item.action(); onClose() }}
          >
            {item.label}
          </button>
        )
      )}
    </div>
  )
}
