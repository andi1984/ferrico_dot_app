import { memo, useLayoutEffect, useRef, useState } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { Bookmark } from '../types'
import { BookmarkCard } from './BookmarkCard'

// Layout constants kept here (not Tailwind) so the virtualizer can do math on them.
const CARD_MIN_WIDTH = 220
const CARD_GAP = 14
const CARD_HEIGHT = 260
const PADDING = 20

interface BookmarkGridProps {
  bookmarks: Bookmark[]
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  onDragPointerDown?: (e: React.PointerEvent, bookmark: Bookmark) => void
}

export function computeColumns(width: number): number {
  if (width < CARD_MIN_WIDTH) return 1
  // n*CARD_MIN_WIDTH + (n-1)*CARD_GAP <= width  →  n <= (width+gap)/(min+gap)
  return Math.max(1, Math.floor((width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)))
}

export const BookmarkGrid = memo(function BookmarkGrid({
  bookmarks,
  onDelete,
  onContext,
  onDragPointerDown,
}: BookmarkGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [columns, setColumns] = useState(1)

  // Track container width so column count adapts on resize.
  useLayoutEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const update = () => setColumns(computeColumns(el.clientWidth - PADDING * 2))
    update()
    if (typeof ResizeObserver === 'undefined') return
    const obs = new ResizeObserver(update)
    obs.observe(el)
    return () => obs.disconnect()
  }, [])

  const rowCount = Math.ceil(bookmarks.length / columns)
  const rowHeight = CARD_HEIGHT + CARD_GAP

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 4,
  })

  // Total content height includes top + bottom padding around the virtual rows.
  const totalSize = virtualizer.getTotalSize()
  const virtualItems = virtualizer.getVirtualItems()

  return (
    <div ref={scrollRef} className="sb-scroll" style={{ height: '100%', overflowY: 'auto', background: 'var(--bg)' }}>
      <div
        style={{
          height: totalSize + PADDING * 2,
          position: 'relative',
        }}
      >
        {virtualItems.map((virtualRow) => {
          const start = virtualRow.index * columns
          const end = Math.min(start + columns, bookmarks.length)
          const rowItems: Bookmark[] = []
          for (let i = start; i < end; i++) rowItems.push(bookmarks[i])
          return (
            <div
              key={virtualRow.key}
              data-grid-row
              style={{
                position: 'absolute',
                top: PADDING,
                left: PADDING,
                right: PADDING,
                transform: `translateY(${virtualRow.start}px)`,
                display: 'grid',
                gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                gap: `${CARD_GAP}px`,
                height: CARD_HEIGHT,
                // Tells the browser to skip painting rows offscreen during fast scrolls.
                contentVisibility: 'auto',
                containIntrinsicSize: `${CARD_HEIGHT}px`,
              }}
            >
              {rowItems.map((bookmark) => (
                <BookmarkCard
                  key={bookmark.id}
                  bookmark={bookmark}
                  onDelete={onDelete}
                  onContext={onContext}
                  onDragPointerDown={onDragPointerDown}
                />
              ))}
            </div>
          )
        })}
      </div>
    </div>
  )
})
