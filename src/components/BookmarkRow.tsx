import { memo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { domainOf, formatDate } from '../utils'
import { Favicon } from './Favicon'
import { IconClose } from './icons'

interface BookmarkRowProps {
  bookmark: Bookmark
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
}

export const BookmarkRow = memo(function BookmarkRow({ bookmark, onDelete, onContext }: BookmarkRowProps) {
  function openUrl(e: React.MouseEvent | React.KeyboardEvent) {
    e.preventDefault()
    invoke('open_url', { url: bookmark.url }).catch(() => {})
  }

  return (
    <div
      className="group relative flex items-center gap-4 px-6 py-3 border-b transition-colors duration-150 hover:bg-[var(--bg-elevated)]"
      style={{ borderColor: 'var(--border-dim)' }}
      onContextMenu={(e) => onContext(e, bookmark)}
    >
      {/* Left accent bar — CSS-driven, no useState */}
      <div
        className="absolute left-0 top-2 bottom-2 w-0.5 rounded-full opacity-0 group-hover:opacity-100 transition-opacity duration-200"
        style={{ background: 'var(--accent)' }}
        aria-hidden="true"
      />

      <Favicon storedUrl={bookmark.favicon_url} bookmarkUrl={bookmark.url} title={bookmark.title} />

      <div className="flex-1 min-w-0">
        <div className="flex items-baseline gap-2 min-w-0">
          <a
            href={bookmark.url}
            onClick={openUrl}
            className="text-sm font-medium truncate leading-snug transition-colors duration-100 cursor-pointer"
            style={{ color: 'var(--text-primary)' }}
            onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--accent-bright)')}
            onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-primary)')}
          >
            {bookmark.title}
          </a>
          <span
            className="text-xs flex-none hidden sm:block"
            style={{ color: 'var(--text-muted)' }}
            aria-hidden="true"
          >
            {domainOf(bookmark.url)}
          </span>
        </div>
        {bookmark.description && (
          <p className="text-xs mt-0.5 truncate" style={{ color: 'var(--text-secondary)' }}>
            {bookmark.description}
          </p>
        )}
      </div>

      {bookmark.tags.length > 0 && (
        <div className="hidden lg:flex items-center gap-1.5 flex-none" aria-label="Tags">
          {bookmark.tags.slice(0, 2).map((tag) => (
            <span
              key={tag.id}
              className="px-2 py-0.5 rounded text-xs font-medium"
              style={{ background: tag.color + '1a', color: tag.color }}
            >
              {tag.name}
            </span>
          ))}
          {bookmark.tags.length > 2 && (
            <span className="text-xs" style={{ color: 'var(--text-muted)' }} aria-label={`${bookmark.tags.length - 2} more tags`}>
              +{bookmark.tags.length - 2}
            </span>
          )}
        </div>
      )}

      <span
        className="text-xs hidden md:block flex-none w-20 text-right"
        style={{ color: 'var(--text-muted)' }}
      >
        {formatDate(bookmark.created_at)}
      </span>

      <button
        onClick={() => onDelete(bookmark.id)}
        className="p-1 rounded flex-none opacity-0 group-hover:opacity-100 transition-all duration-150 cursor-pointer"
        style={{ color: 'var(--text-muted)' }}
        onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--red)')}
        onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
        aria-label={`Delete ${bookmark.title}`}
      >
        <IconClose size={13} />
      </button>
    </div>
  )
})
