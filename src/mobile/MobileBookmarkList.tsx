import { memo, useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { Bookmark } from '../types'
import { MobileBookmarkListItem } from './MobileBookmarkListItem'

interface MobileBookmarkListProps {
  bookmarks: Bookmark[]
}

const ROW_HEIGHT = 68
const ROW_HEIGHT_WITH_DESC = 88

export const MobileBookmarkList = memo(function MobileBookmarkList({ bookmarks }: MobileBookmarkListProps) {
  const scrollRef = useRef<HTMLDivElement>(null)

  const virtualizer = useVirtualizer({
    count: bookmarks.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => (bookmarks[i].description ? ROW_HEIGHT_WITH_DESC : ROW_HEIGHT),
    overscan: 10,
  })

  return (
    <div ref={scrollRef} className="mobile-list-scroll">
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
            <MobileBookmarkListItem bookmark={bookmarks[virtualRow.index]} />
          </div>
        ))}
      </div>
    </div>
  )
})
