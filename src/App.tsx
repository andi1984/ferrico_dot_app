import { useState, useEffect, useCallback, useRef, useMemo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { subscribeToBookmarkAdded, subscribeToHealthCheckProgress, subscribeToCoverUpdated, subscribeToBackupSync, type UnlistenFn } from './events'
import type { Bookmark, Folder, Tag, Selection, ViewMode, SortKey, SidebarData } from './types'
import { extractErrorMessage, duckduckgoFavicon, domainOf } from './utils'
import { ContextMenu, type CtxMenuState } from './components/ContextMenu'
import { BookmarkList } from './components/BookmarkList'
import { BookmarkGrid } from './components/BookmarkGrid'
import { AddBookmarkModal } from './components/AddBookmarkModal'
import { AddFolderModal } from './components/AddFolderModal'
import { AddTagModal } from './components/AddTagModal'
import { SettingsModal } from './components/SettingsModal'
import { BackupSettingsModal } from './components/BackupSettingsModal'
import { ImportCsvModal } from './components/ImportCsvModal'
import { ImportModal } from './components/ImportModal'
import { InboxSortModal } from './components/InboxSortModal'
import { DeduplicateModal } from './components/DeduplicateModal'
import { AiChatPanel } from './components/AiChatPanel'
import { Sidebar, INBOX_DROP_TARGET, FOLDER_ROOT_DROP_TARGET, type DragKind } from './components/Sidebar'
import { EmptyState } from './components/EmptyState'
import { SearchBox, type SearchBoxHandle } from './components/SearchBox'
import { useDragDrop } from './useDragDrop'
import { IconClose, IconImport, IconPlus, IconLayoutList, IconLayoutGrid, IconSort, IconChevronDown, IconSparkles, IconSun, IconMoon, IconBrokenLink, IconFolder } from './components/icons'

type Theme = 'dark' | 'light'

// Maximum folder nesting depth (1-based). Kept in sync with MAX_FOLDER_DEPTH in
// src-tauri/src/db.rs — the backend is the source of truth and enforces it.
const MAX_FOLDER_DEPTH = 3

type Modal = 'add-bookmark' | 'add-folder' | 'add-tag' | 'settings' | 'backup-settings' | 'import' | 'import-csv' | 'inbox-sort' | 'deduplicate' | null

type ScanProgress = { current: number; total: number }

// ─── Loading skeleton ─────────────────────────────────────────────────────────

function RowSkeleton() {
  return (
    <div className="flex items-center gap-4 px-6 py-3 border-b" style={{ borderColor: 'var(--border-dim)' }}>
      <div className="w-7 h-7 rounded flex-none" style={{ background: 'var(--bg-elevated)' }} />
      <div className="flex-1 flex flex-col gap-2">
        <div className="h-3.5 rounded w-2/5" style={{ background: 'var(--bg-elevated)' }} />
      </div>
      <div className="h-3 rounded w-14 hidden md:block" style={{ background: 'var(--bg-elevated)' }} />
      <div className="w-5 flex-none" />
    </div>
  )
}

function LoadingSkeleton() {
  return (
    <div className="h-full overflow-hidden" aria-busy="true" aria-label="Loading bookmarks">
      {Array.from({ length: 14 }, (_, i) => (
        <RowSkeleton key={i} />
      ))}
    </div>
  )
}

// ─── Sort dropdown ────────────────────────────────────────────────────────────

const SORT_OPTIONS: { key: SortKey; label: string }[] = [
  { key: 'date-desc', label: 'Newest first' },
  { key: 'date-asc', label: 'Oldest first' },
  { key: 'title-asc', label: 'Title A → Z' },
  { key: 'title-desc', label: 'Title Z → A' },
  { key: 'domain-asc', label: 'Domain A → Z' },
]

function SortDropdown({ value, onChange }: { value: SortKey; onChange: (k: SortKey) => void }) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    function onMouseDown(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onMouseDown)
    return () => document.removeEventListener('mousedown', onMouseDown)
  }, [open])

  const current = SORT_OPTIONS.find((o) => o.key === value)!

  return (
    <div ref={ref} className="relative flex-none">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1.5 rounded-lg transition-colors duration-150 cursor-pointer"
        style={{
          height: 32,
          padding: '0 10px',
          background: 'var(--input-bg)',
          border: '1px solid var(--border-soft)',
          color: 'var(--text-2)',
          fontSize: 12,
          fontWeight: 500,
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--btn-hover-bg)')}
        onMouseLeave={(e) => (e.currentTarget.style.background = 'var(--input-bg)')}
        aria-label="Sort bookmarks"
        aria-expanded={open}
        aria-haspopup="listbox"
      >
        <IconSort size={13} />
        <span className="hidden sm:inline">{current.label}</span>
        <IconChevronDown size={11} />
      </button>

      {open && (
        <div
          role="listbox"
          aria-label="Sort order"
          className="absolute right-0 top-full mt-1.5 z-50 rounded-lg overflow-hidden py-1 anim-scale-in"
          style={{
            background: 'var(--bg-elev-strong)',
            border: '1px solid var(--border)',
            boxShadow: '0 12px 32px rgba(0,0,0,0.45)',
            minWidth: '168px',
          }}
        >
          {SORT_OPTIONS.map((o) => (
            <button
              key={o.key}
              role="option"
              aria-selected={o.key === value}
              onClick={() => { onChange(o.key); setOpen(false) }}
              className="w-full flex items-center gap-2.5 px-3 py-2 text-left transition-colors duration-100 cursor-pointer"
              style={{
                fontSize: 12.5,
                color: o.key === value ? 'var(--accent)' : 'var(--text-2)',
                fontWeight: o.key === value ? 600 : 500,
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--row-hover-bg)')}
              onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
            >
              <span
                className="w-1.5 h-1.5 rounded-full flex-none"
                style={{ background: o.key === value ? 'var(--accent)' : 'transparent' }}
              />
              {o.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

// ─── App ──────────────────────────────────────────────────────────────────────

// Stable cache key for the list cache: identifies the exact query behind the
// current view (selection scope + active search text).
function listCacheKey(selection: Selection, search: string): string {
  const base =
    selection.type === 'folder' ? `folder:${selection.id}`
      : selection.type === 'tag' ? `tag:${selection.id}`
        : selection.type
  return `${base}|${search}`
}

// Created once: Intl.Collator is far faster than String.localeCompare called
// per comparison when sorting thousands of rows by title or domain.
const collator = new Intl.Collator()

export default function App() {
  // null = first load not yet complete; [] = loaded, no results
  const [bookmarks, setBookmarks] = useState<Bookmark[] | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [tags, setTags] = useState<Tag[]>([])
  const [selection, setSelection] = useState<Selection>({ type: 'all' })
  const [search, setSearch] = useState('')
  const [totalCount, setTotalCount] = useState(0)
  const [inboxCount, setInboxCount] = useState(0)
  const [binCount, setBinCount] = useState(0)
  const [brokenCount, setBrokenCount] = useState(0)
  const [scanProgress, setScanProgress] = useState<ScanProgress | null>(null)
  const [modal, setModal] = useState<Modal>(null)
  const [error, setError] = useState<string | null>(null)
  const [addHovered, setAddHovered] = useState(false)
  const [ctxMenu, setCtxMenu] = useState<CtxMenuState | null>(null)
  const [csvDropPath, setCsvDropPath] = useState<string | null>(null)
  const searchBoxRef = useRef<SearchBoxHandle>(null)

  const [aiChatOpen, setAiChatOpen] = useState(false)
  const [aiFilter, setAiFilter] = useState<Set<string> | null>(null)

  const [viewMode, setViewMode] = useState<ViewMode>(() =>
    (localStorage.getItem('ferrico:viewMode') as ViewMode) ?? 'list'
  )
  const [sortKey, setSortKey] = useState<SortKey>(() =>
    (localStorage.getItem('ferrico:sortKey') as SortKey) ?? 'date-desc'
  )
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem('ferrico:theme') as Theme | null
    return stored === 'light' ? 'light' : 'dark'
  })

  useEffect(() => { localStorage.setItem('ferrico:viewMode', viewMode) }, [viewMode])
  useEffect(() => { localStorage.setItem('ferrico:sortKey', sortKey) }, [sortKey])
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    localStorage.setItem('ferrico:theme', theme)
  }, [theme])

  // Clear the AI filter when the view changes or a new (debounced) search applies.
  useEffect(() => { setAiFilter(null) }, [selection, search])

  // Per-view list cache: last fetched rows keyed by selection+search, cleared on
  // any mutation (see refresh) so it never serves stale rows across edits.
  const listCache = useRef<Map<string, Bookmark[]>>(new Map())

  // Fetch only the visible list. This is the navigation hot path — a single
  // IPC call (and a single DB-mutex lock) so clicking a folder/tag/view paints
  // as soon as that one query returns, instead of waiting on a fan-out of
  // folders + tags + four counts that don't change when you just switch view.
  const loadBookmarks = useCallback(async () => {
    const key = listCacheKey(selection, search)
    // Paint this view's last-known rows instantly, then refetch and reconcile —
    // re-visiting a folder/tag/view feels immediate instead of waiting on IPC.
    const cached = listCache.current.get(key)
    if (cached) setBookmarks(cached)
    try {
      const isBin = selection.type === 'bin'
      const isBroken = selection.type === 'broken'
      const b = await (isBin
        ? invoke<Bookmark[]>('get_bin_bookmarks')
        : isBroken
          ? invoke<Bookmark[]>('get_broken_bookmarks')
          : invoke<Bookmark[]>('get_bookmarks', {
              folderId: selection.type === 'folder' ? selection.id : null,
              tagId: selection.type === 'tag' ? selection.id : null,
              search: search || null,
              inboxOnly: selection.type === 'inbox',
            }))
      listCache.current.set(key, b)
      setBookmarks(b)
      setError(null)
    } catch (e) {
      setError(extractErrorMessage(e))
      // Ensure we exit the loading state even on error
      setBookmarks((prev) => prev ?? [])
    }
  }, [selection, search])

  // Sidebar folder/tag lists + badge counts. Independent of the current view,
  // so it's loaded once on mount and only re-fetched after a mutation — never
  // on plain navigation. One command, one lock (see `get_sidebar` in Rust).
  const loadSidebar = useCallback(async () => {
    try {
      const s = await invoke<SidebarData>('get_sidebar')
      setFolders(s.folders)
      setTags(s.tags)
      setTotalCount(s.counts.total)
      setInboxCount(s.counts.inbox)
      setBinCount(s.counts.bin)
      setBrokenCount(s.counts.broken)
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [])

  // Full refresh after a mutation (add/delete/move/import/…): reconcile both the
  // visible list and the sidebar.
  const refresh = useCallback(() => {
    listCache.current.clear() // any edit invalidates every cached view
    loadBookmarks()
    loadSidebar()
  }, [loadBookmarks, loadSidebar])

  // Optimistically drop rows from the visible list so deletes/moves feel
  // instant; `refresh()` afterwards reconciles counts and the true result.
  const removeLocal = useCallback((ids: string[]) => {
    const drop = new Set(ids)
    setBookmarks((prev) => (prev ? prev.filter((b) => !drop.has(b.id)) : prev))
  }, [])

  useEffect(() => { loadBookmarks() }, [loadBookmarks])
  useEffect(() => { loadSidebar() }, [loadSidebar])

  // Purge bin items older than 30 days on startup
  useEffect(() => {
    invoke('purge_expired_bin', { days: 30 }).catch(() => {})
  }, [])

  // Reload when browser extension adds a bookmark via the HTTP API. The handler
  // reads the latest `refresh` from a ref so the Tauri listener is registered
  // exactly once, not torn down and re-created on every navigation.
  const refreshRef = useRef(refresh)
  useEffect(() => { refreshRef.current = refresh }, [refresh])
  useEffect(() => {
    let active = true
    let unlisten: UnlistenFn | undefined
    subscribeToBookmarkAdded(() => refreshRef.current())
      .then((fn) => {
        if (active) unlisten = fn
        else fn()
      })
      .catch((e) => console.error('[ferrico] bookmark-added listener failed:', e))
    return () => {
      active = false
      unlisten?.()
    }
  }, [])

  // Live cover updates from background scanner
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

  // Google Drive backup sync. A `pull` that replaces local data refreshes the
  // visible list; `backupSyncing` drives a small in-progress indicator. The
  // listener reads `refresh` from the ref so it registers exactly once.
  const [backupSyncing, setBackupSyncing] = useState(false)
  useEffect(() => {
    let active = true
    let unlisten: UnlistenFn | undefined
    subscribeToBackupSync({
      onSyncing: () => setBackupSyncing(true),
      onSynced: ({ op, changed }) => {
        setBackupSyncing(false)
        if (op === 'pull' && changed) refreshRef.current()
      },
      onError: () => setBackupSyncing(false),
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

  // Global keyboard shortcuts
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const mod = e.metaKey || e.ctrlKey
      if (modal) return
      if (mod && e.key === 'n') { e.preventDefault(); setModal('add-bookmark') }
      if (mod && e.key === 'f') { e.preventDefault(); searchBoxRef.current?.focus() }
      if (mod && e.key === ',') { e.preventDefault(); setModal('settings') }
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [modal])

  const sortedBookmarks = useMemo(() => {
    if (!bookmarks) return null
    const base = aiFilter ? bookmarks.filter((b) => aiFilter.has(b.id)) : bookmarks
    // While searching, the backend returns results in fuzzy-relevance order;
    // preserve it. The sort dropdown only applies when there's no active query.
    if (search) return base
    const arr = [...base]
    switch (sortKey) {
      case 'date-desc': return arr.sort((a, b) => b.created_at - a.created_at)
      case 'date-asc':  return arr.sort((a, b) => a.created_at - b.created_at)
      case 'title-asc': return arr.sort((a, b) => collator.compare(a.title, b.title))
      case 'title-desc': return arr.sort((a, b) => collator.compare(b.title, a.title))
      case 'domain-asc': {
        // Precompute each domain once — domainOf() parses a URL, so calling it
        // inside the comparator would re-parse it O(n log n) times.
        return arr
          .map((b) => ({ b, key: domainOf(b.url) }))
          .sort((x, y) => collator.compare(x.key, y.key))
          .map((x) => x.b)
      }
      default: return arr
    }
  }, [bookmarks, sortKey, search, aiFilter])

  const handleAddBookmark = useCallback(async (data: {
    url: string; title: string; description: string
    folder_id: string | null; tag_ids: string[]; feed_url: string | null
  }) => {
    try {
      await invoke('add_bookmark', { input: { ...data, favicon_url: duckduckgoFavicon(data.url) || null } })
      setModal(null)
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [refresh])

  const handleDeleteBookmark = useCallback(async (id: string) => {
    removeLocal([id]) // optimistic — row vanishes immediately
    try {
      await invoke('delete_bookmark', { id })
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
      refresh() // failed → bring the row back
    }
  }, [refresh, removeLocal])

  // Parent folder for the next "New Folder"/"New Subfolder" submission. null =
  // top-level. Set by the folder context menu's "New Subfolder" action.
  const [addFolderParentId, setAddFolderParentId] = useState<string | null>(null)

  const closeModal = useCallback(() => {
    setModal(null)
    setAddFolderParentId(null)
  }, [])

  const handleAddFolder = useCallback(async (name: string) => {
    try {
      await invoke('add_folder', { name, parentId: addFolderParentId })
      closeModal()
      loadSidebar()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadSidebar, addFolderParentId, closeModal])

  const handleDeleteFolder = useCallback(async (id: string) => {
    try {
      await invoke('delete_folder', { id })
      if (selection.type === 'folder' && selection.id === id) setSelection({ type: 'all' })
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [refresh, selection])

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
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [refresh, selection])

  const handleRestoreBookmark = useCallback(async (id: string) => {
    removeLocal([id]) // optimistic — leaves the current (bin) view
    try {
      await invoke('restore_bookmark', { id })
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
      refresh()
    }
  }, [refresh, removeLocal])

  const handleDeleteBookmarkForever = useCallback(async (id: string) => {
    removeLocal([id]) // optimistic
    try {
      await invoke('permanently_delete_bookmark', { id })
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
      refresh()
    }
  }, [refresh, removeLocal])

  const handleEmptyBin = useCallback(async () => {
    try {
      await invoke('empty_bin')
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [refresh])

  const handleScanBrokenBookmarks = useCallback(async () => {
    if (scanProgress) return
    let unlisten: UnlistenFn | undefined
    try {
      setScanProgress({ current: 0, total: 0 })
      unlisten = await subscribeToHealthCheckProgress(setScanProgress)
      await invoke('scan_broken_bookmarks')
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
    } finally {
      setScanProgress(null)
      unlisten?.()
    }
  }, [scanProgress, refresh])

  const handleMoveAllBrokenToBin = useCallback(async () => {
    // Guard: only operate on the broken view — bookmarks state is shared across views
    // and could be stale from a concurrent refresh if the selection changed.
    if (!bookmarks || selection.type !== 'broken') return
    const ids = bookmarks.filter((b) => b.is_broken).map((b) => b.id)
    if (ids.length === 0) return
    removeLocal(ids) // optimistic — broken view empties at once
    try {
      await invoke('delete_bookmarks', { ids })
      refresh()
    } catch (e) {
      setError(extractErrorMessage(e))
      refresh()
    }
  }, [bookmarks, selection, refresh, removeLocal])

  // Toast shown during/after a bookmark move. Auto-dismisses after 2s.
  const [moveStatus, setMoveStatus] = useState<string | null>(null)
  const moveStatusTimerRef = useRef<number | null>(null)

  const showMoveStatus = useCallback((msg: string, autoDismiss = false) => {
    if (moveStatusTimerRef.current) {
      window.clearTimeout(moveStatusTimerRef.current)
      moveStatusTimerRef.current = null
    }
    setMoveStatus(msg)
    if (autoDismiss) {
      moveStatusTimerRef.current = window.setTimeout(() => setMoveStatus(null), 2000)
    }
  }, [])

  const handleMoveBookmark = useCallback(async (bookmark: Bookmark, targetId: string | null) => {
    if (!targetId) return
    const folderId = targetId === INBOX_DROP_TARGET ? null : targetId
    const destinationName =
      folderId === null ? 'Inbox' : (folders.find((f) => f.id === folderId)?.name ?? 'folder')
    // If the move takes the bookmark out of the view we're looking at, drop it
    // from the list immediately so the drag feels like it lands instantly.
    const leavesView =
      (selection.type === 'inbox' && folderId !== null) ||
      (selection.type === 'folder' && folderId !== selection.id)
    if (leavesView) removeLocal([bookmark.id])
    showMoveStatus(`Moving "${bookmark.title}" to ${destinationName}…`)
    try {
      await invoke('move_bookmark', { id: bookmark.id, folderId })
      showMoveStatus(`Moved "${bookmark.title}" to ${destinationName}`, true)
      refresh()
    } catch (e) {
      setMoveStatus(null)
      setError(extractErrorMessage(e))
      refresh()
    }
  }, [folders, selection, refresh, removeLocal, showMoveStatus])

  const drag = useDragDrop<Bookmark>({ onDrop: handleMoveBookmark })

  const handleMoveFolder = useCallback(async (folder: Folder, targetId: string | null) => {
    // Only folder rows and the Folders header are valid folder drop targets.
    if (!targetId || targetId === INBOX_DROP_TARGET) return
    const parentId = targetId === FOLDER_ROOT_DROP_TARGET ? null : targetId
    // No-op drops: onto itself, or onto the parent it already has.
    if (parentId === folder.id) return
    if (parentId === (folder.parent_id ?? null)) return
    try {
      await invoke('move_folder', { id: folder.id, parentId })
      loadSidebar()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadSidebar])

  const folderDrag = useDragDrop<Folder>({ onDrop: handleMoveFolder })

  // Combine the two drag sources for the sidebar: only one is ever active at a
  // time. dragKind lets the sidebar light up only the targets that accept it.
  const dragKind: DragKind = drag.state.active ? 'bookmark' : folderDrag.state.active ? 'folder' : null
  const dragHoverTargetId = drag.state.active
    ? drag.state.hoverTargetId
    : folderDrag.state.active
      ? folderDrag.state.hoverTargetId
      : null

  const openBookmarkContext = useCallback((e: React.MouseEvent, bookmark: Bookmark) => {
    e.preventDefault()
    setCtxMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: 'Open in Browser', action: () => invoke('open_url', { url: bookmark.url }).catch(() => {}) },
        { label: 'Copy URL', action: () => navigator.clipboard.writeText(bookmark.url) },
        { label: 'Copy Title', action: () => navigator.clipboard.writeText(bookmark.title) },
        { sep: true, label: '', action: () => {} },
        { label: 'Move to Bin', danger: true, action: () => handleDeleteBookmark(bookmark.id) },
      ],
    })
  }, [handleDeleteBookmark])

  const openBinBookmarkContext = useCallback((e: React.MouseEvent, bookmark: Bookmark) => {
    e.preventDefault()
    setCtxMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: 'Open in Browser', action: () => invoke('open_url', { url: bookmark.url }).catch(() => {}) },
        { label: 'Copy URL', action: () => navigator.clipboard.writeText(bookmark.url) },
        { sep: true, label: '', action: () => {} },
        { label: 'Restore', action: () => handleRestoreBookmark(bookmark.id) },
        { label: 'Delete Forever', danger: true, action: () => handleDeleteBookmarkForever(bookmark.id) },
      ],
    })
  }, [handleRestoreBookmark, handleDeleteBookmarkForever])

  const openFolderContext = useCallback((e: React.MouseEvent, folder: Folder) => {
    e.preventDefault()
    // 1-based depth: walk parent_id up to the root. Subfolders are capped at
    // MAX_FOLDER_DEPTH levels (kept in sync with db.rs), so a folder already at
    // the max can't host a subfolder.
    const byId = new Map(folders.map((f) => [f.id, f]))
    let depth = 1
    let cur: Folder | undefined = folder
    while (cur?.parent_id) {
      depth += 1
      cur = byId.get(cur.parent_id)
      if (depth > MAX_FOLDER_DEPTH) break
    }
    const items: CtxMenuState['items'] = []
    if (depth < MAX_FOLDER_DEPTH) {
      items.push({
        label: 'New Subfolder',
        action: () => { setAddFolderParentId(folder.id); setModal('add-folder') },
      })
      items.push({ sep: true, label: '', action: () => {} })
    }
    items.push({ label: 'Delete Folder', danger: true, action: () => handleDeleteFolder(folder.id) })
    setCtxMenu({ x: e.clientX, y: e.clientY, items })
  }, [folders, handleDeleteFolder])

  const handleTagClick = useCallback((tagId: string) => {
    setSelection({ type: 'tag', id: tagId })
  }, [])

  const openTagContext = useCallback((e: React.MouseEvent, tag: Tag) => {
    e.preventDefault()
    setCtxMenu({ x: e.clientX, y: e.clientY, items: [{ label: 'Delete Tag', danger: true, action: () => handleDeleteTag(tag.id) }] })
  }, [handleDeleteTag])

  function selectionTitle(): string {
    if (selection.type === 'all') return 'All Bookmarks'
    if (selection.type === 'inbox') return 'Inbox'
    if (selection.type === 'bin') return 'Bin'
    if (selection.type === 'broken') return 'Broken Links'
    if (selection.type === 'folder') return folders.find((f) => f.id === selection.id)?.name ?? 'Folder'
    return tags.find((t) => t.id === selection.id)?.name ?? 'Tag'
  }

  const loading = sortedBookmarks === null
  const isBinView = selection.type === 'bin'
  const isBrokenView = selection.type === 'broken'

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden" style={{ background: 'var(--chrome-bg)', color: 'var(--text-1)' }}>
      <div className="flex-1 flex min-h-0">
      <Sidebar
        folders={folders}
        tags={tags}
        selection={selection}
        bookmarkCount={totalCount}
        inboxCount={inboxCount}
        binCount={binCount}
        brokenCount={brokenCount}
        onSelect={setSelection}
        onAddFolder={() => setModal('add-folder')}
        onDeleteFolder={handleDeleteFolder}
        onAddTag={() => setModal('add-tag')}
        onDeleteTag={handleDeleteTag}
        onOpenSettings={() => setModal('settings')}
        onFolderContext={openFolderContext}
        onTagContext={openTagContext}
        onFolderPointerDown={(e, folder) => folderDrag.startDrag(e, folder)}
        dragHoverTargetId={dragHoverTargetId}
        dragKind={dragKind}
      />

      <div className="flex-1 flex flex-col min-w-0 overflow-hidden" style={{ background: 'var(--bg)' }}>
        {error && (
          <div
            role="alert"
            className="flex items-center justify-between px-6 py-2 gap-4 flex-none text-sm"
            style={{ background: 'rgba(224,82,82,0.1)', borderBottom: '1px solid rgba(224,82,82,0.2)', color: '#e07070' }}
          >
            <p className="truncate">{error}</p>
            <button onClick={() => setError(null)} className="text-xs flex-none hover:opacity-70 transition-opacity cursor-pointer" aria-label="Dismiss error">Dismiss</button>
          </div>
        )}

        <header
          className="flex items-center gap-2.5 px-5 py-3 flex-none flex-wrap"
          style={{ borderBottom: '1px solid var(--border-soft)', background: 'var(--header-bg)' }}
        >
          <div className="flex items-center gap-2 shrink-0 mr-auto flex-wrap">
            <h1
              style={{
                fontFamily: 'var(--font-display)',
                fontSize: 20,
                fontWeight: 600,
                color: 'var(--text-1)',
                letterSpacing: '-0.015em',
              }}
            >{selectionTitle()}</h1>
            {!loading && (
              <span
                className="mono tabnum"
                style={{ fontSize: 11.5, color: 'var(--text-3)', fontWeight: 500 }}
                aria-label={`${sortedBookmarks?.length ?? 0} results`}
              >{(sortedBookmarks?.length ?? 0).toLocaleString()}</span>
            )}
            {aiFilter && (
              <button
                onClick={() => setAiFilter(null)}
                className="flex items-center gap-1 rounded-full cursor-pointer transition-opacity hover:opacity-70"
                style={{
                  padding: '2px 8px',
                  fontSize: 11,
                  fontWeight: 500,
                  background: 'var(--accent-dim)',
                  border: '1px solid var(--accent)',
                  color: 'var(--accent)',
                  lineHeight: 1.6,
                }}
                aria-label="Clear AI filter"
              >
                <IconSparkles size={10} />
                AI filter
                <IconClose size={9} />
              </button>
            )}
          </div>

          {/* Search — owns its per-keystroke state so typing doesn't re-render App */}
          <SearchBox ref={searchBoxRef} onSearch={setSearch} />

          <SortDropdown value={sortKey} onChange={setSortKey} />

          {/* Segmented view-mode pill */}
          <div
            className="flex items-center rounded-lg p-0.5 shrink-0"
            style={{
              background: 'var(--input-bg)',
              border: '1px solid var(--border-soft)',
            }}
            role="group"
            aria-label="View mode"
          >
            {(['list', 'grid'] as ViewMode[]).map((mode) => {
              const isSel = viewMode === mode
              return (
                <button
                  key={mode}
                  onClick={() => setViewMode(mode)}
                  className="flex items-center justify-center rounded-md transition-colors cursor-pointer"
                  style={{
                    width: 26,
                    height: 24,
                    background: isSel ? 'var(--bg-elev-strong)' : 'transparent',
                    color: isSel ? 'var(--text-1)' : 'var(--text-3)',
                    boxShadow: isSel ? '0 1px 2px rgba(0,0,0,0.3)' : 'none',
                  }}
                  aria-label={mode === 'list' ? 'List view' : 'Grid view'}
                  aria-pressed={isSel}
                >
                  {mode === 'list' ? <IconLayoutList size={13} /> : <IconLayoutGrid size={13} />}
                </button>
              )
            })}
          </div>

          <button
            onClick={() => setTheme((t) => (t === 'dark' ? 'light' : 'dark'))}
            className="flex items-center justify-center rounded-lg transition-colors duration-150 flex-none cursor-pointer"
            style={{
              width: 32,
              height: 32,
              background: 'var(--input-bg)',
              border: '1px solid var(--border-soft)',
              color: 'var(--text-1)',
            }}
            onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--btn-hover-bg)')}
            onMouseLeave={(e) => (e.currentTarget.style.background = 'var(--input-bg)')}
            aria-label={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
            title={theme === 'dark' ? 'Light theme' : 'Dark theme'}
          >
            {theme === 'dark' ? <IconSun size={13} /> : <IconMoon size={13} />}
          </button>

          {isBinView ? (
            binCount > 0 && (
              <button
                onClick={handleEmptyBin}
                className="flex items-center gap-1.5 rounded-lg transition-colors duration-150 flex-none cursor-pointer"
                style={{
                  height: 32,
                  padding: '0 11px',
                  background: 'var(--input-bg)',
                  border: '1px solid var(--border-soft)',
                  color: 'var(--red)',
                  fontSize: 12,
                  fontWeight: 500,
                }}
                onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--red)')}
                onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-soft)')}
                aria-label="Empty bin"
              >
                Empty Bin
              </button>
            )
          ) : isBrokenView ? (
            <>
              {scanProgress ? (
                <span
                  className="flex items-center gap-1.5 mono"
                  style={{ fontSize: 12, color: 'var(--text-3)', fontWeight: 500 }}
                  aria-live="polite"
                  aria-label={`Scanning ${scanProgress.current} of ${scanProgress.total}`}
                >
                  <IconBrokenLink size={13} />
                  {scanProgress.total > 0
                    ? `Checking ${scanProgress.current}/${scanProgress.total}…`
                    : 'Starting scan…'}
                </span>
              ) : (
                <button
                  onClick={handleScanBrokenBookmarks}
                  className="flex items-center gap-1.5 rounded-lg transition-colors duration-150 flex-none cursor-pointer"
                  style={{
                    height: 32,
                    padding: '0 11px',
                    background: 'var(--input-bg)',
                    border: '1px solid var(--border-soft)',
                    color: 'var(--text-2)',
                    fontSize: 12,
                    fontWeight: 500,
                  }}
                  onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--btn-hover-bg)')}
                  onMouseLeave={(e) => (e.currentTarget.style.background = 'var(--input-bg)')}
                  aria-label="Scan all bookmarks for broken links"
                >
                  <IconBrokenLink size={13} />
                  Scan Now
                </button>
              )}
              {(sortedBookmarks?.length ?? 0) > 0 && !scanProgress && (
                <button
                  onClick={handleMoveAllBrokenToBin}
                  className="flex items-center gap-1.5 rounded-lg transition-colors duration-150 flex-none cursor-pointer"
                  style={{
                    height: 32,
                    padding: '0 11px',
                    background: 'var(--input-bg)',
                    border: '1px solid var(--border-soft)',
                    color: 'var(--red)',
                    fontSize: 12,
                    fontWeight: 500,
                  }}
                  onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--red)')}
                  onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-soft)')}
                  aria-label="Move all broken bookmarks to bin"
                >
                  Move All to Bin
                </button>
              )}
            </>
          ) : (
            <>
              {selection.type === 'inbox' && (bookmarks?.length ?? 0) > 0 && (
                <button
                  onClick={() => setModal('inbox-sort')}
                  className="flex items-center gap-1.5 rounded-lg transition-colors duration-150 flex-none cursor-pointer"
                  style={{
                    height: 32,
                    padding: '0 11px',
                    border: '1px solid var(--accent)',
                    color: 'var(--accent)',
                    fontSize: 12,
                    fontWeight: 500,
                  }}
                  onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--accent-dim)')}
                  onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
                  aria-label="Sort inbox with AI"
                >
                  <IconSparkles />
                  AI Sort
                </button>
              )}

              <button
                onClick={() => setAiChatOpen((v) => !v)}
                className="flex items-center gap-1.5 rounded-lg transition-colors duration-150 flex-none cursor-pointer"
                style={{
                  height: 32,
                  padding: '0 11px',
                  background: aiChatOpen ? 'var(--accent-dim)' : 'var(--input-bg)',
                  border: `1px solid ${aiChatOpen ? 'var(--accent)' : 'var(--border-soft)'}`,
                  color: aiChatOpen ? 'var(--accent)' : 'var(--text-1)',
                  fontSize: 12,
                  fontWeight: 500,
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = aiChatOpen ? 'var(--accent-dim)' : 'var(--btn-hover-bg)'
                  if (!aiChatOpen) e.currentTarget.style.borderColor = 'var(--accent)'
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = aiChatOpen ? 'var(--accent-dim)' : 'var(--input-bg)'
                  if (!aiChatOpen) e.currentTarget.style.borderColor = 'var(--border-soft)'
                }}
                aria-label="AI search"
                aria-pressed={aiChatOpen}
              >
                <IconSparkles size={12} />
                Ask AI
              </button>

              <button
                onClick={() => setModal('import')}
                className="flex items-center gap-1.5 rounded-lg transition-colors duration-150 flex-none cursor-pointer"
                style={{
                  height: 32,
                  padding: '0 11px',
                  background: 'var(--input-bg)',
                  border: '1px solid var(--border-soft)',
                  color: 'var(--text-1)',
                  fontSize: 12,
                  fontWeight: 500,
                  whiteSpace: 'nowrap',
                }}
                onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--btn-hover-bg)')}
                onMouseLeave={(e) => (e.currentTarget.style.background = 'var(--input-bg)')}
                aria-label="Import bookmarks"
              >
                <IconImport size={13} />
                Import
              </button>

              <button
                onClick={() => setModal('add-bookmark')}
                onMouseEnter={() => setAddHovered(true)}
                onMouseLeave={() => setAddHovered(false)}
                className="btn-accent rounded-lg flex items-center gap-1.5 flex-none cursor-pointer"
                style={{
                  height: 32,
                  padding: '0 12px',
                  fontSize: 12,
                  opacity: addHovered ? 0.95 : 1,
                }}
                aria-label="Add bookmark"
                aria-keyshortcuts="Control+N Meta+N"
              >
                <IconPlus size={13} />
                Add
              </button>
            </>
          )}
        </header>

        {/* Main content — flex-1 + min-h-0 gives the list/grid a bounded, scrollable height */}
        <main className="flex-1 min-h-0">
          {loading ? (
            <LoadingSkeleton />
          ) : sortedBookmarks.length === 0 ? (
            isBinView
              ? <div className="flex items-center justify-center h-full" style={{ color: 'var(--text-muted)' }}>
                  <p className="text-sm">Bin is empty</p>
                </div>
              : isBrokenView
                ? <div className="flex flex-col items-center justify-center h-full gap-3" style={{ color: 'var(--text-muted)' }}>
                    <p className="text-sm">No broken links found</p>
                    <p className="text-xs" style={{ color: 'var(--text-3)' }}>Use "Scan Now" to check all bookmarks</p>
                  </div>
                : <EmptyState onAdd={() => setModal('add-bookmark')} />
          ) : viewMode === 'grid' ? (
            <BookmarkGrid
              bookmarks={sortedBookmarks}
              onDelete={isBinView ? handleDeleteBookmarkForever : handleDeleteBookmark}
              onContext={isBinView ? openBinBookmarkContext : openBookmarkContext}
              onTagClick={handleTagClick}
              onDragPointerDown={!isBinView && !isBrokenView ? drag.startDrag : undefined}
            />
          ) : (
            <BookmarkList
              bookmarks={sortedBookmarks}
              onDelete={isBinView ? handleDeleteBookmarkForever : handleDeleteBookmark}
              onContext={isBinView ? openBinBookmarkContext : openBookmarkContext}
              onTagClick={handleTagClick}
              isBinView={isBinView}
              onRestore={handleRestoreBookmark}
              onDragPointerDown={!isBinView && !isBrokenView ? drag.startDrag : undefined}
            />
          )}
        </main>
      </div>

      {aiChatOpen && bookmarks && (
        <AiChatPanel
          allBookmarks={bookmarks}
          folders={folders}
          onResults={(ids) => setAiFilter(new Set(ids))}
          onClose={() => setAiChatOpen(false)}
        />
      )}
      </div>

      {modal === 'add-bookmark' && (
        <AddBookmarkModal folders={folders} tags={tags} onAdd={handleAddBookmark} onClose={() => setModal(null)} onCreateTag={handleCreateTag} getRelatedTags={getRelatedTags} />
      )}
      {modal === 'add-folder' && (
        <AddFolderModal
          onAdd={handleAddFolder}
          onClose={closeModal}
          parentName={addFolderParentId ? folders.find((f) => f.id === addFolderParentId)?.name : undefined}
        />
      )}
      {modal === 'add-tag' && (
        <AddTagModal onAdd={handleAddTag} onClose={() => setModal(null)} />
      )}
      {modal === 'settings' && (
        <SettingsModal
          onClose={() => setModal(null)}
          onClear={() => { setModal(null); refresh() }}
          onDone={refresh}
          onImportCsv={() => { setModal('import-csv') }}
          onDeduplicate={() => setModal('deduplicate')}
          onBackup={() => setModal('backup-settings')}
        />
      )}
      {modal === 'backup-settings' && (
        <BackupSettingsModal
          onClose={() => setModal('settings')}
          onDone={refresh}
        />
      )}
      {modal === 'import' && (
        <ImportModal
          onClose={() => setModal(null)}
          onDone={refresh}
          onImportCsv={(path) => { setCsvDropPath(path ?? null); setModal('import-csv') }}
        />
      )}
      {modal === 'import-csv' && (
        <ImportCsvModal
          onClose={() => setModal(null)}
          onDone={refresh}
          csvDropPath={csvDropPath}
          onCsvDropConsumed={() => setCsvDropPath(null)}
        />
      )}
      {modal === 'inbox-sort' && bookmarks && (
        <InboxSortModal
          bookmarks={bookmarks}
          folders={folders}
          onClose={() => setModal(null)}
          onDone={refresh}
        />
      )}

      {modal === 'deduplicate' && (
        <DeduplicateModal
          onClose={() => setModal(null)}
          onDone={refresh}
        />
      )}

      {ctxMenu && <ContextMenu state={ctxMenu} onClose={() => setCtxMenu(null)} />}

      {/* Cloud backup sync indicator */}
      {backupSyncing && (
        <div
          className="fixed bottom-4 right-4 z-[90] flex items-center gap-2 rounded-lg px-3 py-2 shadow-lg text-xs"
          style={{ background: 'var(--bg-elev-strong)', border: '1px solid var(--border-soft)', color: 'var(--text-2)' }}
        >
          <span
            className="inline-block w-3 h-3 rounded-full border-2 animate-spin flex-none"
            style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
          />
          Syncing backup…
        </div>
      )}

      {/* Drag ghost — follows the pointer while dragging a bookmark. */}
      {drag.state.active && drag.state.payload && (
        <div
          aria-hidden="true"
          className="fixed z-[100] rounded-lg px-3 py-2 shadow-2xl"
          style={{
            top: drag.state.pointerY + 12,
            left: drag.state.pointerX + 12,
            pointerEvents: 'none',
            background: 'var(--bg-elev-strong)',
            color: 'var(--text-1)',
            border: '1px solid var(--accent)',
            maxWidth: '240px',
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            opacity: 0.95,
            fontSize: 12,
            fontWeight: 500,
          }}
        >
          {drag.state.payload.title}
        </div>
      )}

      {/* Drag ghost — follows the pointer while dragging a folder. */}
      {folderDrag.state.active && folderDrag.state.payload && (
        <div
          aria-hidden="true"
          className="fixed z-[100] rounded-lg px-3 py-2 shadow-2xl flex items-center gap-2"
          style={{
            top: folderDrag.state.pointerY + 12,
            left: folderDrag.state.pointerX + 12,
            pointerEvents: 'none',
            background: 'var(--bg-elev-strong)',
            color: 'var(--text-1)',
            border: '1px solid var(--accent)',
            maxWidth: '240px',
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            opacity: 0.95,
            fontSize: 12,
            fontWeight: 500,
          }}
        >
          <IconFolder size={13} />
          {folderDrag.state.payload.name}
        </div>
      )}

      {/* Move-status toast — shown during and briefly after a move. */}
      {moveStatus && (
        <div
          role="status"
          aria-live="polite"
          className="fixed bottom-5 left-1/2 -translate-x-1/2 z-[110] rounded-lg px-4 py-2.5 shadow-2xl anim-fade-up"
          style={{
            background: 'var(--bg-elev-strong)',
            color: 'var(--text-1)',
            border: '1px solid var(--border)',
            fontSize: 13,
            fontWeight: 500,
            boxShadow: '0 12px 32px rgba(0,0,0,0.4)',
          }}
        >
          {moveStatus}
        </div>
      )}
    </div>
  )
}
