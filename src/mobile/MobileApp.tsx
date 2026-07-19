import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { subscribeToBackupSync, subscribeToCoverUpdated, type UnlistenFn } from '../events'
import type { Bookmark, Counts, Folder, SidebarData, Tag, ViewMode } from '../types'
import { domainOf, extractErrorMessage } from '../utils'
import { IconArrowLeft } from '../components/icons'
import { MobileHeader } from './MobileHeader'
import { FilterDrawer } from './FilterDrawer'
import './mobile.css'

type Theme = 'dark' | 'light'
type Screen = 'browse' | 'settings'

// Mobile navigation scope — read-only v1 has no inbox/bin/broken views.
export type MobileSelection =
  | { type: 'all' }
  | { type: 'folder'; id: string }
  | { type: 'tag'; id: string }

// ─── Loading skeleton (mirrors the desktop LoadingSkeleton in App.tsx) ────────

function RowSkeleton() {
  return (
    <div className="flex flex-col gap-2 px-4 py-3 border-b" style={{ borderColor: 'var(--border-dim)' }}>
      <div className="h-3.5 rounded w-3/5" style={{ background: 'var(--bg-elevated)' }} />
      <div className="h-3 rounded w-2/5" style={{ background: 'var(--bg-elevated)' }} />
    </div>
  )
}

function LoadingSkeleton() {
  return (
    <div className="h-full overflow-hidden" aria-busy="true" aria-label="Loading bookmarks">
      {Array.from({ length: 10 }, (_, i) => (
        <RowSkeleton key={i} />
      ))}
    </div>
  )
}

// ─── MobileApp ────────────────────────────────────────────────────────────────

// Read-only mobile shell. Mirrors App.tsx's data flow using only `get_bookmarks`
// and `get_sidebar` — the mobile UI performs zero DB mutations (no
// `purge_expired_bin`) and never subscribes to `bookmark-added` (the extension
// HTTP server doesn't run on mobile). Snapshots arrive via the Rust sync engine,
// surfaced here through the backup-sync events.
export function MobileApp() {
  // null = first load not yet complete; [] = loaded, no results
  const [bookmarks, setBookmarks] = useState<Bookmark[] | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [tags, setTags] = useState<Tag[]>([])
  const [counts, setCounts] = useState<Counts>({ total: 0, inbox: 0, bin: 0, broken: 0 })
  const [selection, setSelection] = useState<MobileSelection>({ type: 'all' })
  const [search, setSearch] = useState('')
  const [screen, setScreen] = useState<Screen>('browse')
  const [error, setError] = useState<string | null>(null)
  const [syncing, setSyncing] = useState(false)
  const [filterOpen, setFilterOpen] = useState(false)

  const [viewMode, setViewMode] = useState<ViewMode>(() =>
    (localStorage.getItem('ferrico:mobile:viewMode') as ViewMode) ?? 'list'
  )
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem('ferrico:theme') as Theme | null
    return stored === 'light' ? 'light' : 'dark'
  })

  useEffect(() => { localStorage.setItem('ferrico:mobile:viewMode', viewMode) }, [viewMode])
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    localStorage.setItem('ferrico:theme', theme)
  }, [theme])

  const loadBookmarks = useCallback(async () => {
    try {
      const b = await invoke<Bookmark[]>('get_bookmarks', {
        folderId: selection.type === 'folder' ? selection.id : null,
        tagId: selection.type === 'tag' ? selection.id : null,
        search: search || null,
        inboxOnly: false,
      })
      setBookmarks(b)
      setError(null)
    } catch (e) {
      setError(extractErrorMessage(e))
      // Ensure we exit the loading state even on error
      setBookmarks((prev) => prev ?? [])
    }
  }, [selection, search])

  const loadSidebar = useCallback(async () => {
    try {
      const s = await invoke<SidebarData>('get_sidebar')
      setFolders(s.folders)
      setTags(s.tags)
      setCounts(s.counts)
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [])

  useEffect(() => { loadBookmarks() }, [loadBookmarks])
  useEffect(() => { loadSidebar() }, [loadSidebar])

  // The reload handler reads through a ref so the Tauri listeners below are
  // registered exactly once, not torn down on every navigation (App.tsx pattern).
  const reload = useCallback(() => {
    loadBookmarks()
    loadSidebar()
  }, [loadBookmarks, loadSidebar])
  const reloadRef = useRef(reload)
  useEffect(() => { reloadRef.current = reload }, [reload])

  // Sync engine applied a new snapshot → reflect it. Mobile sync is compile-time
  // pull-only, so `changed` always means new local data.
  useEffect(() => {
    let active = true
    let unlisten: UnlistenFn | undefined
    subscribeToBackupSync({
      onSyncing: () => setSyncing(true),
      onSynced: ({ changed }) => {
        setSyncing(false)
        if (changed) reloadRef.current()
      },
      onError: ({ message }) => {
        setSyncing(false)
        setError(message)
      },
    })
      .then((fn) => {
        if (active) unlisten = fn
        else fn()
      })
      .catch((e) => console.error('[ferrico] backup-sync listener failed:', e))
    return () => {
      active = false
      unlisten?.()
    }
  }, [])

  // Live cover updates from the sync-applied snapshot's background fetches
  useEffect(() => {
    let active = true
    let unlisten: UnlistenFn | undefined
    subscribeToCoverUpdated(({ id, cover_url }) => {
      setBookmarks((prev) =>
        prev
          ? prev.map((b) => (b.id === id ? { ...b, cover_url } : b))
          : prev
      )
    })
      .then((fn) => {
        if (active) unlisten = fn
        else fn()
      })
      .catch((e) => console.error('[ferrico] cover-updated listener failed:', e))
    return () => {
      active = false
      unlisten?.()
    }
  }, [])

  const openBookmark = (b: Bookmark) => {
    invoke('open_url', { url: b.url }).catch(() => {})
  }

  // ─── Settings screen (placeholder — pairing import lands in #69) ────────────

  if (screen === 'settings') {
    return (
      <div className="mobile-shell">
        <header className="mobile-chrome">
          <div className="flex items-center gap-2 px-3" style={{ height: 52 }}>
            <button
              onClick={() => setScreen('browse')}
              aria-label="Back"
              className="mobile-icon-btn"
            >
              <IconArrowLeft size={18} />
            </button>
            <h1 className="text-base font-semibold" style={{ fontFamily: 'var(--font-display)' }}>Settings</h1>
          </div>
        </header>
        <main className="mobile-content">
          <p className="px-4 py-6 text-sm" style={{ color: 'var(--text-2)' }}>
            Pairing and sync settings are coming soon.
          </p>
        </main>
      </div>
    )
  }

  // ─── Browse screen ───────────────────────────────────────────────────────────

  return (
    <div className="mobile-shell">
      <MobileHeader
        onSearch={setSearch}
        viewMode={viewMode}
        onToggleView={() => setViewMode((v) => (v === 'list' ? 'grid' : 'list'))}
        theme={theme}
        onToggleTheme={() => setTheme((t) => (t === 'dark' ? 'light' : 'dark'))}
        onOpenSettings={() => setScreen('settings')}
        onOpenFilter={() => setFilterOpen(true)}
        syncing={syncing}
      />

      <FilterDrawer
        open={filterOpen}
        onClose={() => setFilterOpen(false)}
        folders={folders}
        tags={tags}
        counts={counts}
        selection={selection}
        onSelect={setSelection}
      />

      {error && (
        <div
          role="alert"
          className="px-4 py-2 text-xs"
          style={{ background: 'var(--accent-dim)', color: 'var(--red)' }}
        >
          {error}
        </div>
      )}

      <main className="mobile-content">
        {bookmarks === null ? (
          <LoadingSkeleton />
        ) : bookmarks.length === 0 ? (
          <div className="anim-fade-in flex flex-col items-center justify-center h-full gap-2 text-center px-8">
            <p className="font-semibold text-base" style={{ color: 'var(--text-1)' }}>
              {search || selection.type !== 'all' ? 'No bookmarks match' : 'Your library is empty'}
            </p>
            <p className="text-sm" style={{ color: 'var(--text-2)' }}>
              {search || selection.type !== 'all'
                ? 'Try a different search or filter.'
                : 'Pair this device with your desktop to sync your bookmarks.'}
            </p>
          </div>
        ) : viewMode === 'grid' ? (
          /* Placeholder grid — the readOnly BookmarkGrid lands in #68 */
          <div className="mobile-grid">
            {bookmarks.map((b) => (
              <button key={b.id} className="mobile-card" onClick={() => openBookmark(b)}>
                {b.cover_url && <img className="mobile-card-cover" src={b.cover_url} alt="" loading="lazy" />}
                <span className="px-2.5 text-sm font-medium line-clamp-2" style={{ color: 'var(--text-1)' }}>
                  {b.title || b.url}
                </span>
                <span className="px-2.5 text-xs" style={{ color: 'var(--text-3)' }}>
                  {domainOf(b.url)}
                </span>
              </button>
            ))}
          </div>
        ) : (
          /* Placeholder list — the virtualized list view lands in #67 */
          <div>
            {bookmarks.map((b) => (
              <button key={b.id} className="mobile-row" onClick={() => openBookmark(b)}>
                <span className="text-sm font-medium truncate" style={{ color: 'var(--text-1)' }}>
                  {b.title || b.url}
                </span>
                <span className="text-xs" style={{ color: 'var(--text-3)' }}>
                  {domainOf(b.url)}
                </span>
              </button>
            ))}
          </div>
        )}
      </main>
    </div>
  )
}
