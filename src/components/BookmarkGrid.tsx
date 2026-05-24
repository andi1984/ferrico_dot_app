import { useRef, useMemo, useEffect, useState, useCallback } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { Bookmark } from '../types'
import { BookmarkCard } from './BookmarkCard'

interface BookmarkGridProps {
  bookmarks: Bookmark[]
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
}

const MIN_COL_WIDTH = 272  // 260px card + 12px gap allowance
const GAP = 12
const PADDING = 20

// Estimates card height from data so the virtualizer can pre-size rows
// without measuring DOM nodes. Avoids the initial explosion of 2k+ renders.
function estimateCardHeight(b: Bookmark): number {
  let h = 96  // padding (32) + favicon row (28) + margin (12) + date (16) + border
  h += Math.min(3, Math.ceil(b.title.length / 32)) * 20 + 8  // title lines + mb-2
  if (b.description) h += Math.min(4, Math.ceil(b.description.length / 48)) * 18 + 12
  if (b.tags.length > 0) h += 28  // tags row + mb-3
  return h
}

export function BookmarkGrid({ bookmarks, onDelete, onContext }: BookmarkGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [colCount, setColCount] = useState(3)

  // Recompute column count when the scroll container resizes
  useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    const ro = new ResizeObserver(([entry]) => {
      const available = entry.contentRect.width - PADDING * 2
      setColCount(Math.max(1, Math.floor((available + GAP) / (MIN_COL_WIDTH + GAP))))
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // Slice bookmarks into rows of colCount items
  const rows = useMemo<Bookmark[][]>(() => {
    const result: Bookmark[][] = []
    for (let i = 0; i < bookmarks.length; i += colCount) {
      result.push(bookmarks.slice(i, i + colCount))
    }
    return result
  }, [bookmarks, colCount])

  // Stable ref so the estimateSize closure always reads the latest rows
  const rowsRef = useRef(rows)
  rowsRef.current = rows

  const estimateSize = useCallback((i: number) => {
    const row = rowsRef.current[i]
    if (!row?.length) return 160 + GAP
    return Math.max(...row.map(estimateCardHeight)) + GAP
  }, [])

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize,
    measureElement: (el) => el.getBoundingClientRect().height,
    overscan: 3,
  })

  return (
    <div ref={scrollRef} style={{ height: '100%', overflowY: 'auto' }}>
      <div style={{ position: 'relative', height: virtualizer.getTotalSize() + PADDING * 2 }}>
        {virtualizer.getVirtualItems().map((vRow) => (
          <div
            key={vRow.key}
            data-index={vRow.index}
            ref={virtualizer.measureElement}
            style={{
              position: 'absolute',
              top: vRow.start + PADDING,
              left: PADDING,
              right: PADDING,
              display: 'grid',
              gridTemplateColumns: `repeat(${colCount}, minmax(0, 1fr))`,
              gap: GAP,
              paddingBottom: GAP,
            }}
          >
            {rows[vRow.index].map((bookmark) => (
              <BookmarkCard
                key={bookmark.id}
                bookmark={bookmark}
                onDelete={onDelete}
                onContext={onContext}
              />
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}
