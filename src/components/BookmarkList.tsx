import { useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { Bookmark } from '../types'
import { BookmarkRow } from './BookmarkRow'

interface BookmarkListProps {
  bookmarks: Bookmark[]
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  isBinView?: boolean
  onRestore?: (id: string) => void
}

export function BookmarkList({ bookmarks, onDelete, onContext, isBinView, onRestore }: BookmarkListProps) {
  const scrollRef = useRef<HTMLDivElement>(null)

  const virtualizer = useVirtualizer({
    count: bookmarks.length,
    getScrollElement: () => scrollRef.current,
    // Description rows are ~64px, plain rows ~48px — close enough for stable layout
    estimateSize: (i) => (bookmarks[i]?.description ? 64 : 48),
    overscan: 10,
  })

  return (
    <div ref={scrollRef} style={{ height: '100%', overflowY: 'auto' }}>
      <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
        {virtualizer.getVirtualItems().map((virtualRow) => (
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
            <BookmarkRow
              bookmark={bookmarks[virtualRow.index]}
              onDelete={onDelete}
              onContext={onContext}
              isBinView={isBinView}
              onRestore={onRestore}
            />
          </div>
        ))}
      </div>
    </div>
  )
}
