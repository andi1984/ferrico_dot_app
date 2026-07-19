import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { subscribeToBackupSync, subscribeToCoverUpdated, type UnlistenFn } from '../events'
import type { Bookmark, Counts, Folder, SidebarData, Tag, ViewMode } from '../types'
import { extractErrorMessage } from '../utils'
import { MobileHeader } from './MobileHeader'
import { FilterDrawer } from './FilterDrawer'
import { MobileBookmarkList } from './MobileBookmarkList'
import { BookmarkGrid } from '../components/BookmarkGrid'
import { MobileSettings } from './MobileSettings'
import './mobile.css'

type Theme = 'dark' | 'light'
type Screen = 'browse' | 'settings'

// Foreground-resume pull cooldown — avoids hammering backup_sync_now every
// time the user briefly switches away and back. Exported for the test.
export const FOREGROUND_SYNC_MIN_INTERVAL_MS = 10 * 60 * 1000

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

  // Cooldown clock for the foreground-resume sync below — reset whenever any
  // sync cycle completes (launch pull, manual refresh, or foreground-resume
  // itself), not just by the resume trigger, so they don't pile up.
  const lastSyncAttemptRef = useRef(Date.now())

  // Sync engine applied a new snapshot → reflect it. Mobile sync is compile-time
  // pull-only, so `changed` always means new local data.
  useEffect(() => {
    let active = true
    let unlisten: UnlistenFn | undefined
    subscribeToBackupSync({
      onSyncing: () => setSyncing(true),
      onSynced: ({ changed }) => {
        setSyncing(false)
        lastSyncAttemptRef.current = Date.now()
        if (changed) reloadRef.current()
      },
      onError: ({ message }) => {
        setSyncing(false)
        lastSyncAttemptRef.current = Date.now()
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

  // Foreground resume: the app already gets a pull on launch (Rust-side
  // open-pull, both platforms) and a manual pull via the header's refresh
  // button — this adds a third trigger for the common "leave app, come back"
  // case, without touching Rust (see #70 for why: cheap, testable, with the
  // native `RunEvent::Resumed` noted as an upgrade path if this proves
  // unreliable on-device). Only fires when paired (`backup_status().enabled`)
  // so unpaired users never see a spurious "not connected" error banner.
  useEffect(() => {
    function onVisibilityChange() {
      if (document.visibilityState !== 'visible') return
      if (Date.now() - lastSyncAttemptRef.current < FOREGROUND_SYNC_MIN_INTERVAL_MS) return
      lastSyncAttemptRef.current = Date.now()
      invoke<{ enabled: boolean } | null>('backup_status')
        .then((status) => {
          if (status?.enabled) return invoke('backup_sync_now')
        })
        .catch(() => {})
    }
    document.addEventListener('visibilitychange', onVisibilityChange)
    return () => document.removeEventListener('visibilitychange', onVisibilityChange)
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

  // ─── Settings screen ─────────────────────────────────────────────────────────

  if (screen === 'settings') {
    return (
      <MobileSettings
        onClose={() => setScreen('browse')}
        theme={theme}
        onToggleTheme={() => setTheme((t) => (t === 'dark' ? 'light' : 'dark'))}
      />
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
          <BookmarkGrid bookmarks={bookmarks} readOnly />
        ) : (
          <MobileBookmarkList bookmarks={bookmarks} />
        )}
      </main>
    </div>
  )
}
