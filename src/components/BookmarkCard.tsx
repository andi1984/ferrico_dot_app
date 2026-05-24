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

export const BookmarkCard = memo(function BookmarkCard({ bookmark, onDelete, onContext, onDragPointerDown }: BookmarkCardProps) {
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
      className="group relative rounded-xl p-4 mb-3 break-inside-avoid transition-shadow duration-150 cursor-grab select-none"
      style={{
        background: 'var(--bg-elevated)',
        border: '1px solid var(--border-dim)',
        boxShadow: '0 1px 3px rgba(0,0,0,0.12)',
        touchAction: 'none',
      }}
      onMouseEnter={(e) => {
        const el = e.currentTarget as HTMLDivElement
        el.style.borderColor = 'var(--border-mid)'
        el.style.boxShadow = '0 4px 20px rgba(0,0,0,0.22)'
      }}
      onMouseLeave={(e) => {
        const el = e.currentTarget as HTMLDivElement
        el.style.borderColor = 'var(--border-dim)'
        el.style.boxShadow = '0 1px 3px rgba(0,0,0,0.12)'
      }}
      onContextMenu={(e) => onContext(e, bookmark)}
      onPointerDown={handlePointerDown}
    >
      <button
        onClick={() => onDelete(bookmark.id)}
        className="absolute top-3 right-3 p-1 rounded opacity-0 group-hover:opacity-100 transition-all duration-150 cursor-pointer"
        style={{ color: 'var(--text-muted)' }}
        onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--red)')}
        onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
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
        className="block text-sm font-medium leading-snug mb-2 cursor-pointer transition-colors duration-100"
        style={{
          color: 'var(--text-primary)',
          display: '-webkit-box',
          WebkitLineClamp: 3,
          WebkitBoxOrient: 'vertical',
          overflow: 'hidden',
        } as React.CSSProperties}
        onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--accent-bright)')}
        onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-primary)')}
      >
        {bookmark.title}
      </a>

      {bookmark.description && (
        <p
          className="text-xs mb-3 leading-relaxed"
          style={{
            color: 'var(--text-secondary)',
            display: '-webkit-box',
            WebkitLineClamp: 4,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
          } as React.CSSProperties}
        >
          {bookmark.description}
        </p>
      )}

      {bookmark.tags.length > 0 && (
        <div className="flex flex-wrap gap-1 mb-3">
          {bookmark.tags.map((tag) => (
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

      <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
        {formatDate(bookmark.created_at)}
      </span>
    </div>
  )
})
