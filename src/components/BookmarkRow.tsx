import { memo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { domainOf, formatDate } from '../utils'
import { Favicon } from './Favicon'
import { IconClose, IconRestore, IconAlertTriangle } from './icons'

interface BookmarkRowProps {
  bookmark: Bookmark
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  onTagClick?: (tagId: string) => void
  isBinView?: boolean
  onRestore?: (id: string) => void
  onDragPointerDown?: (e: React.PointerEvent, bookmark: Bookmark) => void
}

export const BookmarkRow = memo(function BookmarkRow({ bookmark, onDelete, onContext, onTagClick, isBinView, onRestore, onDragPointerDown }: BookmarkRowProps) {
  function openUrl(e: React.MouseEvent | React.KeyboardEvent) {
    e.preventDefault()
    invoke('open_url', { url: bookmark.url }).catch(() => {})
  }

  // Drag from anywhere on the row except interactive elements (link, buttons,
  // tags) so those keep their normal click behavior without the 5px wiggle.
  function handlePointerDown(e: React.PointerEvent) {
    const target = e.target as HTMLElement
    if (target.closest('a, button, [data-no-drag]')) return
    onDragPointerDown?.(e, bookmark)
  }

  return (
    <div
      className="group relative cursor-grab select-none transition-colors"
      style={{ touchAction: 'none' }}
      onContextMenu={(e) => onContext(e, bookmark)}
      onPointerDown={handlePointerDown}
      onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--row-hover-bg-strong)')}
      onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
    >
      <div
        className="flex items-start gap-3.5 px-5 py-3"
        style={{ borderBottom: '1px solid var(--border-soft)' }}
      >
        <Favicon
          storedUrl={bookmark.favicon_url}
          bookmarkUrl={bookmark.url}
          title={bookmark.title}
          size={38}
          radius={8}
        />

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-0.5 min-w-0">
            <a
              href={bookmark.url}
              onClick={openUrl}
              className="truncate transition-colors duration-100 cursor-pointer"
              style={{
                fontSize: 14.5,
                fontWeight: 600,
                color: 'var(--text-1)',
                letterSpacing: '-0.005em',
              }}
              onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--accent-bright)')}
              onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-1)')}
            >
              {bookmark.title}
            </a>
            {bookmark.is_broken && (
              <span
                className="flex-none flex items-center"
                style={{ color: 'var(--red)' }}
                title="Broken link — URL returned an error or is unreachable"
                aria-label="Broken link"
              >
                <IconAlertTriangle size={12} />
              </span>
            )}
          </div>

          {bookmark.description && (
            <p
              className="truncate"
              style={{
                fontSize: 13,
                color: 'var(--text-2)',
                lineHeight: 1.45,
                marginBottom: 6,
              }}
            >
              {bookmark.description}
            </p>
          )}

          <div className="flex items-center gap-2.5 mt-1" style={{ minHeight: 18 }}>
            <span
              className="mono truncate"
              style={{ fontSize: 11, color: 'var(--text-3)', maxWidth: '50%' }}
            >
              {domainOf(bookmark.url)}
            </span>
            {bookmark.tags.slice(0, 3).map((tag) => (
              <button
                key={tag.id}
                type="button"
                data-no-drag
                onClick={(e) => { e.stopPropagation(); onTagClick?.(tag.id) }}
                className="tag-pill cursor-pointer truncate transition-colors duration-100"
                style={{ background: tag.color + '22', color: tag.color, maxWidth: 96, border: 'none' }}
                onMouseEnter={(e) => (e.currentTarget.style.background = tag.color + '38')}
                onMouseLeave={(e) => (e.currentTarget.style.background = tag.color + '22')}
                aria-label={`Filter by tag ${tag.name}`}
                title={`Filter by tag: ${tag.name}`}
              >
                {tag.name}
              </button>
            ))}
            {bookmark.tags.length > 3 && (
              <span
                className="mono"
                style={{ fontSize: 10.5, color: 'var(--text-3)' }}
                aria-label={`${bookmark.tags.length - 3} more tags`}
              >+{bookmark.tags.length - 3}</span>
            )}
            <span className="ml-auto flex items-center gap-2">
              <span
                className="mono tabnum"
                style={{ fontSize: 11, color: 'var(--text-3)' }}
              >
                {formatDate(isBinView && bookmark.deleted_at ? bookmark.deleted_at : bookmark.created_at)}
              </span>
            </span>
          </div>
        </div>

        {/* Hover actions (right-anchored). Gradient mask hides underlying date/tags. */}
        <div
          className="absolute right-5 top-1/2 -translate-y-1/2 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-150"
          style={{
            paddingLeft: 24,
            background: 'linear-gradient(90deg, transparent 0%, var(--bg) 30%)',
          }}
          data-no-drag
        >
          {isBinView ? (
            <>
              <button
                onClick={() => onRestore?.(bookmark.id)}
                className="flex items-center justify-center rounded-md transition-colors duration-100 cursor-pointer"
                style={{ width: 28, height: 28, color: 'var(--text-2)' }}
                onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--btn-hover-bg)'; e.currentTarget.style.color = 'var(--accent)' }}
                onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-2)' }}
                aria-label={`Restore ${bookmark.title}`}
              >
                <IconRestore size={13} />
              </button>
              <button
                onClick={() => onDelete(bookmark.id)}
                className="flex items-center justify-center rounded-md transition-colors duration-100 cursor-pointer"
                style={{ width: 28, height: 28, color: 'var(--text-2)' }}
                onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--btn-hover-bg)'; e.currentTarget.style.color = 'var(--red)' }}
                onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-2)' }}
                aria-label={`Delete ${bookmark.title} permanently`}
              >
                <IconClose size={13} />
              </button>
            </>
          ) : (
            <button
              onClick={() => onDelete(bookmark.id)}
              className="flex items-center justify-center rounded-md transition-colors duration-100 cursor-pointer"
              style={{ width: 28, height: 28, color: 'var(--text-2)' }}
              onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--btn-hover-bg)'; e.currentTarget.style.color = 'var(--red)' }}
              onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-2)' }}
              aria-label={`Delete ${bookmark.title}`}
            >
              <IconClose size={13} />
            </button>
          )}
        </div>
      </div>
    </div>
  )
})
