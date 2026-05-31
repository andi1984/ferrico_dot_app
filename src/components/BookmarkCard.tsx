import { memo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { domainOf, formatDate, initials } from '../utils'
import { Favicon } from './Favicon'
import { IconClose } from './icons'

interface BookmarkCardProps {
  bookmark: Bookmark
  onDelete: (id: string) => void
  onContext: (e: React.MouseEvent, bookmark: Bookmark) => void
  onTagClick?: (tagId: string) => void
  onDragPointerDown?: (e: React.PointerEvent, bookmark: Bookmark) => void
}

// Stable preview gradients keyed off the URL so each tile reads as its own.
const PREVIEW_GRADIENTS: Array<[string, string]> = [
  ['#2a221d', '#1a1612'], // base copper
  ['#1c2a26', '#0f1715'], // teal night
  ['#2a1a1c', '#180f10'], // ember
  ['#1f2233', '#11131e'], // indigo
  ['#2a261a', '#181508'], // amber dusk
  ['#1a2820', '#0d1612'], // moss
]
function hash(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0
  return Math.abs(h)
}

export const BookmarkCard = memo(function BookmarkCard({
  bookmark,
  onDelete,
  onContext,
  onTagClick,
  onDragPointerDown,
}: BookmarkCardProps) {
  function openUrl(e: React.MouseEvent | React.KeyboardEvent) {
    e.preventDefault()
    invoke('open_url', { url: bookmark.url }).catch(() => {})
  }

  function handlePointerDown(e: React.PointerEvent) {
    const target = e.target as HTMLElement
    if (target.closest('a, button, [data-no-drag]')) return
    onDragPointerDown?.(e, bookmark)
  }

  const [from, to] = PREVIEW_GRADIENTS[hash(bookmark.url) % PREVIEW_GRADIENTS.length]
  const glyph = initials(bookmark.title)

  return (
    <div
      className="bm-card group relative cursor-grab select-none flex flex-col"
      onContextMenu={(e) => onContext(e, bookmark)}
      onPointerDown={handlePointerDown}
    >
      {/* Preview banner */}
      <div
        className="relative shrink-0"
        style={{
          aspectRatio: '16 / 9',
          background: `linear-gradient(135deg, ${from} 0%, ${to} 100%)`,
          borderBottom: '1px solid var(--border-soft)',
        }}
      >
        <div
          className="absolute inset-0 flex items-center justify-center"
          style={{ opacity: 0.18 }}
          aria-hidden="true"
        >
          <span
            style={{
              fontFamily: 'var(--font-display)',
              fontSize: 72,
              fontWeight: 700,
              color: 'rgba(255,255,255,0.85)',
              letterSpacing: '-0.04em',
            }}
          >{glyph}</span>
        </div>

        <div className="absolute bottom-2.5 left-2.5">
          <Favicon
            storedUrl={bookmark.favicon_url}
            bookmarkUrl={bookmark.url}
            title={bookmark.title}
            size={28}
            radius={6}
          />
        </div>

        <button
          onClick={() => onDelete(bookmark.id)}
          className="bm-card-close absolute top-2.5 right-2.5 rounded-md opacity-0 group-hover:opacity-100 transition-opacity duration-150 cursor-pointer flex items-center justify-center"
          style={{
            width: 22,
            height: 22,
            background: 'rgba(0,0,0,0.35)',
            backdropFilter: 'blur(6px)',
          }}
          aria-label={`Delete ${bookmark.title}`}
          data-no-drag
        >
          <IconClose size={12} />
        </button>
      </div>

      {/* Body */}
      <div className="p-3.5 flex flex-col flex-1 min-h-0">
        <a
          href={bookmark.url}
          onClick={openUrl}
          className="bm-card-title block cursor-pointer"
          style={{
            fontSize: 13.5,
            fontWeight: 600,
            lineHeight: 1.35,
            letterSpacing: '-0.005em',
            display: '-webkit-box',
            WebkitLineClamp: 2,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
            minHeight: '2.7em',
            marginBottom: 4,
          } as React.CSSProperties}
        >
          {bookmark.title}
        </a>

        {bookmark.tags.length > 0 && (
          <div className="flex flex-wrap gap-1 mb-2" data-no-drag>
            {bookmark.tags.slice(0, 3).map((tag) => (
              <button
                key={tag.id}
                type="button"
                data-no-drag
                onClick={(e) => { e.stopPropagation(); onTagClick?.(tag.id) }}
                className="tag-pill cursor-pointer truncate transition-colors duration-100"
                style={{ background: tag.color + '22', color: tag.color, maxWidth: 88, border: 'none' }}
                onMouseEnter={(e) => (e.currentTarget.style.background = tag.color + '38')}
                onMouseLeave={(e) => (e.currentTarget.style.background = tag.color + '22')}
                aria-label={`Filter by tag ${tag.name}`}
                title={`Filter by tag: ${tag.name}`}
              >
                {tag.name}
              </button>
            ))}
            {bookmark.tags.length > 3 && (
              <span className="mono" style={{ fontSize: 10.5, color: 'var(--text-3)' }}>
                +{bookmark.tags.length - 3}
              </span>
            )}
          </div>
        )}

        <div className="flex items-center gap-2 mt-auto pt-1">
          <span className="mono truncate" style={{ fontSize: 10.5, color: 'var(--text-3)' }}>
            {domainOf(bookmark.url)}
          </span>
          <span className="mono tabnum ml-auto shrink-0" style={{ fontSize: 10.5, color: 'var(--text-3)' }}>
            {formatDate(bookmark.created_at)}
          </span>
        </div>
      </div>
    </div>
  )
})
