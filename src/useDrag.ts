import { useRef, useState, useCallback } from 'react'
import type { Bookmark } from './types'

// Stored in data-drop-target attribute on sidebar items.
// '__inbox__' signals folderId = null (move to inbox).
export const INBOX_DROP_TARGET = '__inbox__'

interface UseDragOptions {
  onDrop: (bookmarkId: string, folderId: string | null) => void
}

interface UseDragResult {
  startDrag: (e: React.MouseEvent, bookmark: Bookmark) => void
  dragTargetId: string | null  // current data-drop-target value, or null
}

// Minimum pixels of movement before a drag is initiated.
const DRAG_THRESHOLD = 5

export function useDrag({ onDrop }: UseDragOptions): UseDragResult {
  const draggingIdRef = useRef<string | null>(null)
  const ghostRef = useRef<HTMLDivElement | null>(null)
  const rafRef = useRef<number>(0)
  const prevTargetRef = useRef<string | null>(null)
  // Only re-renders when the hovered target actually changes (not every mousemove pixel)
  const [dragTargetId, setDragTargetId] = useState<string | null>(null)

  const cleanup = useCallback(() => {
    ghostRef.current?.remove()
    ghostRef.current = null
    draggingIdRef.current = null
    cancelAnimationFrame(rafRef.current)
    prevTargetRef.current = null
    setDragTargetId(null)
    document.body.style.cursor = ''
    document.body.style.userSelect = ''
  }, [])

  // Temporarily hides the ghost, reads elementFromPoint, restores ghost.
  const findDropTarget = useCallback((x: number, y: number): HTMLElement | null => {
    const ghost = ghostRef.current
    if (!ghost) return null
    ghost.style.display = 'none'
    const el = document.elementFromPoint(x, y)
    ghost.style.display = ''
    return (el?.closest('[data-drop-target]') as HTMLElement) ?? null
  }, [])

  const startDrag = useCallback((e: React.MouseEvent, bookmark: Bookmark) => {
    if (e.button !== 0) return  // left button only
    const startX = e.clientX
    const startY = e.clientY
    let activated = false

    const activate = (x: number, y: number) => {
      activated = true
      draggingIdRef.current = bookmark.id
      document.body.style.userSelect = 'none'
      document.body.style.cursor = 'grabbing'

      const ghost = document.createElement('div')
      Object.assign(ghost.style, {
        position: 'fixed',
        pointerEvents: 'none',
        zIndex: '9999',
        maxWidth: '240px',
        padding: '6px 12px',
        borderRadius: '8px',
        background: 'var(--bg-elevated)',
        border: '1px solid var(--accent)',
        color: 'var(--text-primary)',
        fontSize: '13px',
        fontFamily: 'Outfit, sans-serif',
        fontWeight: '500',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        boxShadow: '0 8px 32px rgba(0,0,0,0.6)',
        opacity: '0.95',
        left: `${x + 14}px`,
        top: `${y - 18}px`,
      })
      ghost.textContent = bookmark.title
      document.body.appendChild(ghost)
      ghostRef.current = ghost
    }

    const onMove = (ev: MouseEvent) => {
      if (!activated) {
        const dist = Math.hypot(ev.clientX - startX, ev.clientY - startY)
        if (dist < DRAG_THRESHOLD) return
        activate(ev.clientX, ev.clientY)
      }

      if (!ghostRef.current) return
      ghostRef.current.style.left = `${ev.clientX + 14}px`
      ghostRef.current.style.top = `${ev.clientY - 18}px`

      // RAF-debounce the React state update so it only fires on target changes
      cancelAnimationFrame(rafRef.current)
      rafRef.current = requestAnimationFrame(() => {
        const target = findDropTarget(ev.clientX, ev.clientY)
        const newId = target?.dataset.dropTarget ?? null
        if (newId !== prevTargetRef.current) {
          prevTargetRef.current = newId
          setDragTargetId(newId)
        }
      })
    }

    const onUp = (ev: MouseEvent) => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseup', onUp)

      if (activated && draggingIdRef.current) {
        const target = findDropTarget(ev.clientX, ev.clientY)
        if (target) {
          const raw = target.dataset.dropTarget!
          const folderId = raw === INBOX_DROP_TARGET ? null : raw
          onDrop(draggingIdRef.current, folderId)
        }
      }

      cleanup()
    }

    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseup', onUp)
  }, [onDrop, cleanup, findDropTarget])

  return { startDrag, dragTargetId }
}
