import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { subscribeToBackupSync, subscribeToCoverUpdated, type UnlistenFn } from '../events'
import type { Bookmark, Counts, Folder, SidebarData, Tag, ViewMode } from '../types'
import { duckduckgoFavicon, extractErrorMessage } from '../utils'
import { MobileHeader } from './MobileHeader'
import { FilterDrawer } from './FilterDrawer'
import { MobileBookmarkList } from './MobileBookmarkList'
import { MobileActionSheet, type SheetAction } from './MobileActionSheet'
import { MobileFolderPicker } from './MobileFolderPicker'
import { BookmarkGrid } from '../components/BookmarkGrid'
import { AddBookmarkModal } from '../components/AddBookmarkModal'
import { AddFolderModal } from '../components/AddFolderModal'
import { AddTagModal } from '../components/AddTagModal'
import { MobileSettings } from './MobileSettings'
import { IconFolder, IconPlus, IconRestore, IconTrash } from '../components/icons'
import './mobile.css'

type Theme = 'dark' | 'light'
type Screen = 'browse' | 'settings'

// Foreground-resume pull cooldown — avoids hammering neon_sync_now every
// time the user briefly switches away and back. Exported for the test.
export const FOREGROUND_SYNC_MIN_INTERVAL_MS = 10 * 60 * 1000

// Kept in sync with MAX_FOLDER_DEPTH in db.rs / App.tsx (1-based).
const MAX_FOLDER_DEPTH = 3

// Mobile navigation scope — no `broken` view: the link scanner stays
// desktop-only (a full URL health sweep on cellular would drain battery for
// little value; broken flags still arrive via sync).
export type MobileSelection =
  | { type: 'all' }
  | { type: 'inbox' }
  | { type: 'bin' }
  | { type: 'folder'; id: string }
  | { type: 'tag'; id: string }

type MobileModal =
  | { kind: 'add-bookmark' }
  | { kind: 'add-folder'; parentId: string | null; parentName?: string }
  | { kind: 'add-tag' }

type MobileSheet =
  | { kind: 'bookmark'; bookmark: Bookmark }
  | { kind: 'folder'; folder: Folder; level: number }
  | { kind: 'tag'; tag: Tag }
  | { kind: 'move'; bookmark: Bookmark }

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

// Read-write mobile shell. Mirrors App.tsx's data flow and mutation handlers
// (add/delete/restore/move bookmark, folder and tag management, bin) minus the
// desktop-only surfaces: AI features (no local `claude` CLI on Android),
// import/export (no mobile file picker), the broken-link scanner, and the
// browser-extension server (so no `bookmark-added` subscription). Local edits
// push to Neon through the Rust change loop; `neon_flush` on backgrounding is
// the Android stand-in for the desktop sync-before-close hook.
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
  const [modal, setModal] = useState<MobileModal | null>(null)
  const [sheet, setSheet] = useState<MobileSheet | null>(null)

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
      const b = await (selection.type === 'bin'
        ? invoke<Bookmark[]>('get_bin_bookmarks')
        : invoke<Bookmark[]>('get_bookmarks', {
            folderId: selection.type === 'folder' ? selection.id : null,
            tagId: selection.type === 'tag' ? selection.id : null,
            search: search || null,
            inboxOnly: selection.type === 'inbox',
          }))
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

  // Expired bin entries purge on startup, same as the desktop shell.
  useEffect(() => {
    invoke('purge_expired_bin').catch(() => {})
  }, [])

  // The reload handler reads through a ref so the Tauri listeners below are
  // registered exactly once, not torn down on every navigation (App.tsx pattern).
  const reload = useCallback(() => {
    loadBookmarks()
    loadSidebar()
  }, [loadBookmarks, loadSidebar])
  const reloadRef = useRef(reload)
  useEffect(() => { reloadRef.current = reload }, [reload])

  // Optimistic removal for delete/restore/move — the row vanishes immediately,
  // and the follow-up reload() reconciles (or brings it back on error).
  const removeLocal = useCallback((ids: string[]) => {
    const gone = new Set(ids)
    setBookmarks((prev) => (prev ? prev.filter((b) => !gone.has(b.id)) : prev))
  }, [])

  // Cooldown clock for the foreground-resume sync below — reset whenever any
  // sync cycle completes (launch pull, manual refresh, or foreground-resume
  // itself), not just by the resume trigger, so they don't pile up.
  const lastSyncAttemptRef = useRef(Date.now())

  // Sync engine finished a cycle → reflect whatever the merge changed locally.
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

  // Foreground resume: pull so remote edits land (cooldown-limited). On
  // backgrounding, flush pending local edits with the best-effort `neon_flush`
  // — Android has no CloseRequested event, so leaving the app is the last
  // reliable moment before the OS may kill the process. The flush is not
  // cooldown-limited: it no-ops server-side when nothing is dirty.
  useEffect(() => {
    function onVisibilityChange() {
      if (document.visibilityState !== 'visible') {
        invoke('neon_flush').catch(() => {})
        return
      }
      if (Date.now() - lastSyncAttemptRef.current < FOREGROUND_SYNC_MIN_INTERVAL_MS) return
      lastSyncAttemptRef.current = Date.now()
      invoke<{ enabled: boolean } | null>('neon_status')
        .then((status) => {
          if (status?.enabled) return invoke('neon_sync_now')
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

  // ─── Mutation handlers (mirroring App.tsx) ───────────────────────────────────

  const handleAddBookmark = useCallback(async (data: {
    url: string; title: string; description: string
    folder_id: string | null; tag_ids: string[]; feed_url: string | null
  }) => {
    try {
      await invoke('add_bookmark', { input: { ...data, favicon_url: duckduckgoFavicon(data.url) || null } })
      setModal(null)
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [reload])

  const handleDeleteBookmark = useCallback(async (id: string) => {
    removeLocal([id]) // optimistic — row vanishes immediately
    try {
      await invoke('delete_bookmark', { id })
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
      reload() // failed → bring the row back
    }
  }, [reload, removeLocal])

  const handleRestoreBookmark = useCallback(async (id: string) => {
    removeLocal([id]) // optimistic — leaves the bin view
    try {
      await invoke('restore_bookmark', { id })
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
      reload()
    }
  }, [reload, removeLocal])

  const handleDeleteBookmarkForever = useCallback(async (id: string) => {
    removeLocal([id]) // optimistic
    try {
      await invoke('permanently_delete_bookmark', { id })
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
      reload()
    }
  }, [reload, removeLocal])

  const handleEmptyBin = useCallback(async () => {
    try {
      await invoke('empty_bin')
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [reload])

  const handleMoveBookmark = useCallback(async (bookmark: Bookmark, folderId: string | null) => {
    const leavesView =
      (selection.type === 'inbox' && folderId !== null) ||
      (selection.type === 'folder' && folderId !== selection.id)
    if (leavesView) removeLocal([bookmark.id])
    try {
      await invoke('move_bookmark', { id: bookmark.id, folderId })
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
      reload()
    }
  }, [selection, reload, removeLocal])

  const handleAddFolder = useCallback(async (name: string) => {
    const parentId = modal?.kind === 'add-folder' ? modal.parentId : null
    try {
      await invoke('add_folder', { name, parentId })
      setModal(null)
      loadSidebar()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [modal, loadSidebar])

  const handleDeleteFolder = useCallback(async (id: string) => {
    try {
      await invoke('delete_folder', { id })
      if (selection.type === 'folder' && selection.id === id) setSelection({ type: 'all' })
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [reload, selection])

  const handleAddTag = useCallback(async (name: string, color: string) => {
    try {
      await invoke('add_tag', { name, color })
      setModal(null)
      loadSidebar()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadSidebar])

  // Inline tag creation from the New Bookmark combobox: persist, refresh the
  // sidebar list, and return the tag so the combobox can select it immediately.
  const handleCreateTag = useCallback(async (name: string, color: string): Promise<Tag> => {
    const tag = await invoke<Tag>('add_tag', { name, color })
    setTags((prev) => (prev.some((t) => t.id === tag.id) ? prev : [...prev, tag]))
    loadSidebar()
    return tag
  }, [loadSidebar])

  const getRelatedTags = useCallback(
    (ids: string[]) => invoke<Tag[]>('related_tags', { tagIds: ids }),
    [],
  )

  const handleDeleteTag = useCallback(async (id: string) => {
    try {
      await invoke('delete_tag', { id })
      if (selection.type === 'tag' && selection.id === id) setSelection({ type: 'all' })
      reload()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [reload, selection])

  // ─── Action sheets ───────────────────────────────────────────────────────────

  const inBin = selection.type === 'bin'
  const openSheet = useCallback((bookmark: Bookmark) => {
    setSheet({ kind: 'bookmark', bookmark })
  }, [])

  function sheetActions(s: MobileSheet): { title: string; actions: SheetAction[] } {
    switch (s.kind) {
      case 'bookmark': {
        const b = s.bookmark
        if (inBin) {
          return {
            title: b.title || b.url,
            actions: [
              { label: 'Restore', icon: <IconRestore size={15} />, onPress: () => handleRestoreBookmark(b.id) },
              { label: 'Delete forever', icon: <IconTrash size={15} />, danger: true, onPress: () => handleDeleteBookmarkForever(b.id) },
            ],
          }
        }
        return {
          title: b.title || b.url,
          actions: [
            { label: 'Move to folder…', icon: <IconFolder size={15} />, onPress: () => setSheet({ kind: 'move', bookmark: b }) },
            { label: 'Move to bin', icon: <IconTrash size={15} />, danger: true, onPress: () => handleDeleteBookmark(b.id) },
          ],
        }
      }
      case 'folder': {
        const actions: SheetAction[] = []
        // `level` is 0-based; a folder at level MAX_FOLDER_DEPTH-1 is at the cap.
        if (s.level < MAX_FOLDER_DEPTH - 1) {
          actions.push({
            label: 'New subfolder…',
            icon: <IconPlus size={15} />,
            onPress: () => setModal({ kind: 'add-folder', parentId: s.folder.id, parentName: s.folder.name }),
          })
        }
        actions.push({
          label: 'Delete folder',
          icon: <IconTrash size={15} />,
          danger: true,
          onPress: () => handleDeleteFolder(s.folder.id),
        })
        return { title: s.folder.name, actions }
      }
      case 'tag':
        return {
          title: s.tag.name,
          actions: [
            { label: 'Delete tag', icon: <IconTrash size={15} />, danger: true, onPress: () => handleDeleteTag(s.tag.id) },
          ],
        }
      case 'move':
        // Rendered as MobileFolderPicker below, not as an action sheet.
        return { title: '', actions: [] }
    }
  }

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
        onAddFolder={() => { setFilterOpen(false); setModal({ kind: 'add-folder', parentId: null }) }}
        onAddTag={() => { setFilterOpen(false); setModal({ kind: 'add-tag' }) }}
        onFolderMenu={(folder, level) => { setFilterOpen(false); setSheet({ kind: 'folder', folder, level }) }}
        onTagMenu={(tag) => { setFilterOpen(false); setSheet({ kind: 'tag', tag }) }}
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

      {inBin && bookmarks !== null && bookmarks.length > 0 && (
        <div className="flex items-center justify-between px-4 py-2" style={{ borderBottom: '1px solid var(--border-dim)' }}>
          <span className="text-xs" style={{ color: 'var(--text-2)' }}>
            Bin — items are deleted forever after 30 days
          </span>
          <button
            className="rounded-lg px-3 cursor-pointer"
            style={{ height: 28, fontSize: 11.5, fontWeight: 500, color: 'var(--red)', background: 'transparent', border: '1px solid var(--border-soft)' }}
            onClick={handleEmptyBin}
          >
            Empty bin
          </button>
        </div>
      )}

      <main className="mobile-content">
        {bookmarks === null ? (
          <LoadingSkeleton />
        ) : bookmarks.length === 0 ? (
          <div className="anim-fade-in flex flex-col items-center justify-center h-full gap-2 text-center px-8">
            <p className="font-semibold text-base" style={{ color: 'var(--text-1)' }}>
              {inBin
                ? 'Bin is empty'
                : search || selection.type !== 'all'
                  ? 'No bookmarks match'
                  : 'Your library is empty'}
            </p>
            <p className="text-sm" style={{ color: 'var(--text-2)' }}>
              {inBin
                ? 'Deleted bookmarks land here for 30 days.'
                : search || selection.type !== 'all'
                  ? 'Try a different search or filter.'
                  : 'Add a bookmark with the + button, or pair with your desktop in Settings to sync.'}
            </p>
          </div>
        ) : viewMode === 'grid' && !inBin ? (
          <BookmarkGrid bookmarks={bookmarks} readOnly onMore={openSheet} />
        ) : (
          // The bin always renders as a list: its rows are management-only
          // (restore / delete forever), which the card layout can't offer.
          <MobileBookmarkList bookmarks={bookmarks} onMore={openSheet} inBin={inBin} />
        )}
      </main>

      {!inBin && (
        <button className="mobile-fab" onClick={() => setModal({ kind: 'add-bookmark' })} aria-label="Add bookmark">
          <IconPlus size={22} />
        </button>
      )}

      {sheet && sheet.kind !== 'move' && (
        <MobileActionSheet open {...sheetActions(sheet)} onClose={() => setSheet(null)} />
      )}
      {sheet?.kind === 'move' && (
        <MobileFolderPicker
          open
          folders={folders}
          onPick={(folderId) => handleMoveBookmark(sheet.bookmark, folderId)}
          onClose={() => setSheet(null)}
        />
      )}

      {modal?.kind === 'add-bookmark' && (
        <AddBookmarkModal
          folders={folders}
          tags={tags}
          onAdd={handleAddBookmark}
          onClose={() => setModal(null)}
          onCreateTag={handleCreateTag}
          getRelatedTags={getRelatedTags}
        />
      )}
      {modal?.kind === 'add-folder' && (
        <AddFolderModal
          onAdd={handleAddFolder}
          onClose={() => setModal(null)}
          parentName={modal.parentName}
        />
      )}
      {modal?.kind === 'add-tag' && (
        <AddTagModal onAdd={handleAddTag} onClose={() => setModal(null)} />
      )}
    </div>
  )
}
