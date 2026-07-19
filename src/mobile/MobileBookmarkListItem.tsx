import { memo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { domainOf } from '../utils'
import { Favicon } from '../components/Favicon'

interface MobileBookmarkListItemProps {
  bookmark: Bookmark
}

// Read-only row for the mobile list view. No drag, no custom context menu, no
// hover affordances — whole row is a single tap target that opens the URL in
// the system browser. Unlike the desktop BookmarkRow, this sets no
// `touchAction` override, so the list scrolls normally under touch.
// `onContextMenu` preventDefaults the *native* long-press menu Chromium's
// Android WebView shows by default — without it, a long-press eats the touch
// before the click ever reaches us (haptic + sound, nothing happens).
export const MobileBookmarkListItem = memo(function MobileBookmarkListItem({ bookmark }: MobileBookmarkListItemProps) {
  function openUrl() {
    invoke('open_url', { url: bookmark.url }).catch(() => {})
  }

  return (
    <button
      type="button"
      onClick={openUrl}
      onContextMenu={(e) => e.preventDefault()}
      className="mobile-list-item select-none"
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
  )
})
