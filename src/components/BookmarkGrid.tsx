import type { Bookmark } from '../types'
import { BookmarkCard } from './BookmarkCard'

interface BookmarkGridProps {
  bookmarks: Bookmark[]
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  onDragPointerDown?: (e: React.PointerEvent, bookmark: Bookmark) => void
}

export function BookmarkGrid({ bookmarks, onDelete, onContext, onDragPointerDown }: BookmarkGridProps) {
  return (
    <div className="h-full overflow-y-auto">
      <div className="p-5" style={{ columns: '260px', gap: '12px' }}>
        {bookmarks.map((bookmark) => (
          <BookmarkCard
            key={bookmark.id}
            bookmark={bookmark}
            onDelete={onDelete}
            onContext={onContext}
            onDragPointerDown={onDragPointerDown}
          />
        ))}
      </div>
    </div>
  )
}
