import type { Bookmark } from '../types'
import { BookmarkCard } from './BookmarkCard'

interface BookmarkGridProps {
  bookmarks: Bookmark[]
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  onDragStart?: (e: React.DragEvent, bookmark: Bookmark) => void
}

export function BookmarkGrid({ bookmarks, onDelete, onContext, onDragStart }: BookmarkGridProps) {
  return (
    <div className="h-full overflow-y-auto">
      <div className="p-5" style={{ columns: '260px', gap: '12px' }}>
        {bookmarks.map((bookmark) => (
          <BookmarkCard
            key={bookmark.id}
            bookmark={bookmark}
            onDelete={onDelete}
            onContext={onContext}
            onDragStart={onDragStart}
          />
        ))}
      </div>
    </div>
  )
}
