import { useCallback, useEffect, useRef, useState } from 'react'

// Pointer-event based drag-and-drop. Works on mouse, trackpad, and touch —
// unlike the HTML5 Drag and Drop API, which is unreliable in Tauri's macOS
// WKWebView (especially with trackpads).
//
// Usage:
//   const drag = useDragDrop<Bookmark>({ onDrop: (bm, targetId) => ... })
//   <Source onPointerDown={(e) => drag.startDrag(e, bookmark)} />
//   <Target data-drop-target-id="folder-123" />
//   drag.state.hoverTargetId  // string of current hovered target's id, or null
//
// The drop-target id is read from the [data-drop-target-id] attribute on the
// nearest ancestor of the element at the pointer position.

const DEFAULT_THRESHOLD = 5
const DROP_TARGET_ATTR = 'data-drop-target-id'

export interface DragState<T> {
  active: boolean
  payload: T | null
  pointerX: number
  pointerY: number
  hoverTargetId: string | null
}

export interface UseDragDropOptions<T> {
  onDrop: (payload: T, targetId: string | null) => void
  threshold?: number
}

export interface UseDragDropReturn<T> {
  state: DragState<T>
  startDrag: (e: React.PointerEvent, payload: T) => void
}

const EMPTY_STATE: DragState<unknown> = {
  active: false,
  payload: null,
  pointerX: 0,
  pointerY: 0,
  hoverTargetId: null,
}

function findDropTargetId(x: number, y: number): string | null {
  const el = document.elementFromPoint(x, y) as HTMLElement | null
  if (!el) return null
  const target = el.closest(`[${DROP_TARGET_ATTR}]`)
  return target?.getAttribute(DROP_TARGET_ATTR) ?? null
}

export function useDragDrop<T>({
  onDrop,
  threshold = DEFAULT_THRESHOLD,
}: UseDragDropOptions<T>): UseDragDropReturn<T> {
  const [state, setState] = useState<DragState<T>>(EMPTY_STATE as DragState<T>)

  // Mutable refs are needed because document-level pointer listeners are
  // attached once but closure over React state would be stale.
  const pendingRef = useRef<{ payload: T; startX: number; startY: number; pointerId: number } | null>(null)
  const activeRef = useRef(false)
  const suppressClickRef = useRef(false)
  // Element holding pointer capture for the current gesture, released on cleanup.
  const captureRef = useRef<{ el: Element; pointerId: number } | null>(null)

  const cleanup = useCallback(() => {
    const cap = captureRef.current
    if (cap) {
      try { (cap.el as HTMLElement).releasePointerCapture?.(cap.pointerId) } catch { /* already released */ }
      captureRef.current = null
    }
    pendingRef.current = null
    activeRef.current = false
    document.body.style.cursor = ''
    document.body.style.userSelect = ''
    setState(EMPTY_STATE as DragState<T>)
  }, [])

  useEffect(() => {
    // rAF-batched pointer position. setState fires at most once per frame,
    // which keeps the App / Sidebar / drag ghost re-render rate at ~60fps
    // even when the OS streams hundreds of pointermove events per second.
    let rafId = 0
    let pendingX = 0
    let pendingY = 0

    function flush() {
      rafId = 0
      const pending = pendingRef.current
      if (!pending) return
      const hoverTargetId = findDropTargetId(pendingX, pendingY)
      setState({
        active: true,
        payload: pending.payload,
        pointerX: pendingX,
        pointerY: pendingY,
        hoverTargetId,
      })
    }

    function onPointerMove(e: PointerEvent) {
      const pending = pendingRef.current
      if (!pending || e.pointerId !== pending.pointerId) return
      const dx = e.clientX - pending.startX
      const dy = e.clientY - pending.startY
      if (!activeRef.current) {
        if (Math.hypot(dx, dy) < threshold) return
        activeRef.current = true
        document.body.style.cursor = 'grabbing'
        document.body.style.userSelect = 'none'
      }
      pendingX = e.clientX
      pendingY = e.clientY
      if (rafId === 0) rafId = requestAnimationFrame(flush)
    }

    function onPointerUp(e: PointerEvent) {
      const pending = pendingRef.current
      if (!pending || e.pointerId !== pending.pointerId) return
      const wasActive = activeRef.current
      if (wasActive) {
        const targetId = findDropTargetId(e.clientX, e.clientY)
        suppressClickRef.current = true
        // Defer so cleanup runs first and consumer can re-read fresh state.
        const payload = pending.payload
        cleanup()
        onDrop(payload, targetId)
      } else {
        cleanup()
      }
    }

    function onPointerCancel(e: PointerEvent) {
      const pending = pendingRef.current
      if (!pending || e.pointerId !== pending.pointerId) return
      cleanup()
    }

    // Swallow the synthetic click that follows a drag, so links don't navigate
    // and buttons don't fire after a drop.
    function onClickCapture(e: MouseEvent) {
      if (suppressClickRef.current) {
        e.preventDefault()
        e.stopPropagation()
        suppressClickRef.current = false
      }
    }

    document.addEventListener('pointermove', onPointerMove)
    document.addEventListener('pointerup', onPointerUp)
    document.addEventListener('pointercancel', onPointerCancel)
    document.addEventListener('click', onClickCapture, true)
    return () => {
      if (rafId !== 0) cancelAnimationFrame(rafId)
      document.removeEventListener('pointermove', onPointerMove)
      document.removeEventListener('pointerup', onPointerUp)
      document.removeEventListener('pointercancel', onPointerCancel)
      document.removeEventListener('click', onClickCapture, true)
    }
  }, [cleanup, onDrop, threshold])

  const startDrag = useCallback((e: React.PointerEvent, payload: T) => {
    // Only left mouse button / primary pointer
    if (e.button !== 0) return
    // Stop the native text-selection from starting on pointerdown — the 5px
    // pre-drag window otherwise highlights anything the pointer passes over.
    e.preventDefault()
    pendingRef.current = {
      payload,
      startX: e.clientX,
      startY: e.clientY,
      pointerId: e.pointerId,
    }
    activeRef.current = false
    // Capture the pointer on the source element. WebKitGTK (Tauri's Linux
    // webview) otherwise stops delivering pointermove/pointerup once the press
    // turns into a drag, so the whole gesture silently dies. Capturing keeps the
    // event stream flowing to this element (and bubbling to our document
    // listeners) regardless of what's under the cursor. elementFromPoint still
    // hit-tests the real drop target, so capture doesn't affect targeting.
    const el = e.currentTarget as HTMLElement
    try {
      el.setPointerCapture?.(e.pointerId)
      captureRef.current = { el, pointerId: e.pointerId }
    } catch { /* capture unsupported — fall back to bubbling */ }
  }, [])

  return { state, startDrag }
}
