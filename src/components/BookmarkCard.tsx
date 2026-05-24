import { memo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { domainOf, formatDate } from '../utils'
import { Favicon } from './Favicon'
import { IconClose } from './icons'

interface BookmarkCardProps {
  bookmark: Bookmark
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  onDragPointerDown?: (e: React.PointerEvent, bookmark: Bookmark) => void
}

export const BookmarkCard = memo(function BookmarkCard({
  bookmark,
  onDelete,
  onContext,
  onDragPointerDown,
}: BookmarkCardProps) {
  function openUrl(e: React.MouseEvent | React.KeyboardEvent) {
    e.preventDefault()
    invoke('open_url', { url: bookmark.url }).catch(() => {})
  }

  // Drag from anywhere on the card except interactive elements (link, buttons,
  // tags) so those keep their normal click behavior without the 5px wiggle.
  function handlePointerDown(e: React.PointerEvent) {
    const target = e.target as HTMLElement
    if (target.closest('a, button, [data-no-drag]')) return
    onDragPointerDown?.(e, bookmark)
  }

  return (
    <div
      className="bm-card group relative rounded-xl p-4 cursor-grab select-none overflow-hidden"
      onContextMenu={(e) => onContext(e, bookmark)}
      onPointerDown={handlePointerDown}
    >
      <button
        onClick={() => onDelete(bookmark.id)}
        className="bm-card-close absolute top-3 right-3 p-1 rounded opacity-0 group-hover:opacity-100 transition-opacity duration-150 cursor-pointer"
        aria-label={`Delete ${bookmark.title}`}
      >
        <IconClose size={12} />
      </button>

      <div className="flex items-center gap-2 mb-3 pr-5">
        <Favicon storedUrl={bookmark.favicon_url} bookmarkUrl={bookmark.url} title={bookmark.title} />
        <span className="text-xs truncate" style={{ color: 'var(--text-muted)' }}>
          {domainOf(bookmark.url)}
        </span>
      </div>

      <a
        href={bookmark.url}
        onClick={openUrl}
        className="bm-card-title block text-sm font-medium leading-snug mb-2 cursor-pointer"
        style={{
          display: '-webkit-box',
          WebkitLineClamp: 3,
          WebkitBoxOrient: 'vertical',
          overflow: 'hidden',
        } as React.CSSProperties}
      >
        {bookmark.title}
      </a>

      {bookmark.description && (
        <p
          className="text-xs mb-3 leading-relaxed"
          style={{
            color: 'var(--text-secondary)',
            display: '-webkit-box',
            WebkitLineClamp: 3,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
          } as React.CSSProperties}
        >
          {bookmark.description}
        </p>
      )}

      {bookmark.tags.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-3">
          {bookmark.tags.slice(0, 4).map((tag) => (
            <span
              key={tag.id}
              data-no-drag
              className="px-2 py-0.5 rounded text-xs font-medium cursor-default"
              style={{ background: tag.color + '1a', color: tag.color }}
            >
              {tag.name}
            </span>
          ))}
        </div>
      )}

      <span
        className="absolute left-4 bottom-3 text-xs"
        style={{ color: 'var(--text-muted)' }}
      >
        {formatDate(bookmark.created_at)}
      </span>
    </div>
  )
})
