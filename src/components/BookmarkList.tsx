import { memo, useMemo, useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { Bookmark } from '../types'
import { BookmarkRow } from './BookmarkRow'

interface BookmarkListProps {
  bookmarks: Bookmark[]
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  onTagClick?: (tagId: string) => void
  isBinView?: boolean
  onRestore?: (id: string) => void
  onDragPointerDown?: (e: React.PointerEvent, bookmark: Bookmark) => void
}

type ListItem =
  | { kind: 'header'; label: string; count: number }
  | { kind: 'bookmark'; bookmark: Bookmark }

const HEADER_HEIGHT = 38

// Buckets a unix-seconds timestamp into a human-friendly group label.
function dateGroup(ts: number, now: number): string {
  const days = Math.floor((now - ts) / 86400)
  if (days <= 0) return 'Today'
  if (days <= 1) return 'Yesterday'
  if (days <= 7) return 'This week'
  if (days <= 14) return 'Last week'
  if (days <= 30) return 'This month'
  return 'Earlier'
}

function buildItems(bookmarks: Bookmark[], isBinView: boolean): ListItem[] {
  const now = Math.floor(Date.now() / 1000)
  const items: ListItem[] = []
  let currentLabel: string | null = null
  let runCount = 0
  let headerIndex = -1
  for (const b of bookmarks) {
    const ts = isBinView && b.deleted_at ? b.deleted_at : b.created_at
    const label = dateGroup(ts, now)
    if (label !== currentLabel) {
      if (headerIndex >= 0) {
        // finalize previous header count
        ;(items[headerIndex] as { kind: 'header'; label: string; count: number }).count = runCount
      }
      currentLabel = label
      runCount = 0
      headerIndex = items.length
      items.push({ kind: 'header', label, count: 0 })
    }
    items.push({ kind: 'bookmark', bookmark: b })
    runCount += 1
  }
  if (headerIndex >= 0) {
    ;(items[headerIndex] as { kind: 'header'; label: string; count: number }).count = runCount
  }
  return items
}

export const BookmarkList = memo(function BookmarkList({ bookmarks, onDelete, onContext, onTagClick, isBinView, onRestore, onDragPointerDown }: BookmarkListProps) {
  const scrollRef = useRef<HTMLDivElement>(null)

  const items = useMemo(() => buildItems(bookmarks, !!isBinView), [bookmarks, isBinView])

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => {
      const it = items[i]
      if (it.kind === 'header') return HEADER_HEIGHT
      return it.bookmark.description ? 88 : 68
    },
    overscan: 10,
  })

  return (
    <div ref={scrollRef} className="sb-scroll" style={{ height: '100%', overflowY: 'auto', background: 'var(--bg)' }}>
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const it = items[virtualRow.index]
          return (
            <div
              key={virtualRow.key}
              data-index={virtualRow.index}
              ref={virtualizer.measureElement}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${virtualRow.start}px)`,
              }}
            >
              {it.kind === 'header' ? (
                <div
                  className="flex items-center gap-3 px-5"
                  style={{
                    height: HEADER_HEIGHT,
                    background: 'var(--bg)',
                  }}
                >
                  <span className="section-label whitespace-nowrap">{it.label}</span>
                  <span className="mono tabnum" style={{ fontSize: 10.5, color: 'var(--text-3)' }}>
                    {it.count}
                  </span>
                  <div className="flex-1 h-px" style={{ background: 'var(--border-soft)' }} />
                </div>
              ) : (
                <BookmarkRow
                  bookmark={it.bookmark}
                  onDelete={onDelete}
                  onContext={onContext}
                  onTagClick={onTagClick}
                  isBinView={isBinView}
                  onRestore={onRestore}
                  onDragPointerDown={onDragPointerDown}
                />
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
})
