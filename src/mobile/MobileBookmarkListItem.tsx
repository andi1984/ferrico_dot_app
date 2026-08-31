import { memo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { domainOf } from '../utils'
import { Favicon } from '../components/Favicon'
import { IconMore } from '../components/icons'

interface MobileBookmarkListItemProps {
  bookmark: Bookmark
  /** Opens the action sheet for this bookmark. Omit to render a plain
      tap-to-open row with no overflow button. */
  onMore?: (bookmark: Bookmark) => void
  /** Bin rows don't open the (deleted) URL on tap — they only offer actions. */
  inBin?: boolean
}

// Row for the mobile list view. No drag, no custom context menu, no hover
// affordances — the main area is a single tap target that opens the URL in
// the system browser; mutations live behind the trailing "⋮" action-sheet
// button. Unlike the desktop BookmarkRow, this sets no `touchAction`
// override, so the list scrolls normally under touch. `onContextMenu`
// preventDefaults the *native* long-press menu Chromium's Android WebView
// shows by default — without it, a long-press eats the touch before the
// click ever reaches us (haptic + sound, nothing happens).
export const MobileBookmarkListItem = memo(function MobileBookmarkListItem({
  bookmark,
  onMore,
  inBin,
}: MobileBookmarkListItemProps) {
  function handleTap() {
    if (inBin) {
      onMore?.(bookmark)
      return
    }
    invoke('open_url', { url: bookmark.url }).catch(() => {})
  }

  return (
    <div className="mobile-list-item select-none" onContextMenu={(e) => e.preventDefault()}>
      <button
        type="button"
        onClick={handleTap}
        className="mobile-list-item-main"
        aria-label={bookmark.title || bookmark.url}
      >
        <Favicon
          storedUrl={bookmark.favicon_url}
          bookmarkUrl={bookmark.url}
          title={bookmark.title}
          size={38}
          radius={8}
        />
        <div className="mobile-list-item-text">
          <span className="mobile-list-item-title">{bookmark.title || bookmark.url}</span>
          {bookmark.description && (
            <span className="mobile-list-item-desc">{bookmark.description}</span>
          )}
          <span className="mobile-list-item-domain">{domainOf(bookmark.url)}</span>
        </div>
      </button>
      {onMore && (
        <button
          type="button"
          className="mobile-icon-btn"
          onClick={() => onMore(bookmark)}
          aria-label={`Actions for ${bookmark.title || bookmark.url}`}
        >
          <IconMore size={18} />
        </button>
      )}
    </div>
  )
})
