import { useState, useEffect, useCallback, useRef, useMemo } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { subscribeToBookmarkAdded, type UnlistenFn } from './events'
import type { Bookmark, Folder, Tag, Selection, ViewMode, SortKey } from './types'
import { extractErrorMessage, duckduckgoFavicon, domainOf } from './utils'
import { ContextMenu, type CtxMenuState } from './components/ContextMenu'
import { BookmarkList } from './components/BookmarkList'
import { BookmarkGrid } from './components/BookmarkGrid'
import { AddBookmarkModal } from './components/AddBookmarkModal'
import { AddFolderModal } from './components/AddFolderModal'
import { AddTagModal } from './components/AddTagModal'
import { SettingsModal } from './components/SettingsModal'
import { ImportCsvModal } from './components/ImportCsvModal'
import { InboxSortModal } from './components/InboxSortModal'
import { Sidebar } from './components/Sidebar'
import { EmptyState } from './components/EmptyState'
import { IconClose, IconPlus, IconSearch, IconLayoutList, IconLayoutGrid, IconSort, IconChevronDown, IconSparkles } from './components/icons'

type Modal = 'add-bookmark' | 'add-folder' | 'add-tag' | 'settings' | 'import-csv' | 'inbox-sort' | null

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
        className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-xs font-medium transition-colors duration-150 cursor-pointer"
        style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
        onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
        onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
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
          className="absolute right-0 top-full mt-1 z-50 rounded-lg overflow-hidden py-1"
          style={{
            background: 'var(--bg-elevated)',
            border: '1px solid var(--border-mid)',
            boxShadow: '0 8px 24px rgba(0,0,0,0.3)',
            minWidth: '148px',
          }}
        >
          {SORT_OPTIONS.map((o) => (
            <button
              key={o.key}
              role="option"
              aria-selected={o.key === value}
              onClick={() => { onChange(o.key); setOpen(false) }}
              className="w-full flex items-center gap-2 px-3 py-2 text-xs text-left transition-colors duration-100 cursor-pointer"
              style={{ color: o.key === value ? 'var(--accent-bright)' : 'var(--text-secondary)' }}
              onMouseEnter={(e) => (e.currentTarget.style.background = 'rgba(255,255,255,0.05)')}
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

// ─── View mode toggle ─────────────────────────────────────────────────────────

function ViewToggle({ value, onChange }: { value: ViewMode; onChange: (m: ViewMode) => void }) {
  return (
    <div
      className="flex items-center rounded-lg overflow-hidden flex-none"
      style={{ border: '1px solid var(--border-mid)' }}
      role="group"
      aria-label="View mode"
    >
      {(['list', 'grid'] as ViewMode[]).map((mode) => (
        <button
          key={mode}
          onClick={() => onChange(mode)}
          className="flex items-center justify-center px-2.5 py-2 transition-colors duration-150 cursor-pointer"
          style={{
            color: value === mode ? 'var(--accent-bright)' : 'var(--text-muted)',
            background: value === mode ? 'rgba(255,255,255,0.07)' : 'transparent',
          }}
          aria-label={mode === 'list' ? 'List view' : 'Grid view'}
          aria-pressed={value === mode}
        >
          {mode === 'list' ? <IconLayoutList size={14} /> : <IconLayoutGrid size={14} />}
        </button>
      ))}
    </div>
  )
}

// ─── App ──────────────────────────────────────────────────────────────────────

export default function App() {
  // null = first load not yet complete; [] = loaded, no results
  const [bookmarks, setBookmarks] = useState<Bookmark[] | null>(null)
  const [folders, setFolders] = useState<Folder[]>([])
  const [tags, setTags] = useState<Tag[]>([])
  const [selection, setSelection] = useState<Selection>({ type: 'all' })
  const [searchInput, setSearchInput] = useState('')
  const [search, setSearch] = useState('')
  const [totalCount, setTotalCount] = useState(0)
  const [inboxCount, setInboxCount] = useState(0)
  const [binCount, setBinCount] = useState(0)
  const [modal, setModal] = useState<Modal>(null)
  const [error, setError] = useState<string | null>(null)
  const [addHovered, setAddHovered] = useState(false)
  const [searchFocused, setSearchFocused] = useState(false)
  const [ctxMenu, setCtxMenu] = useState<CtxMenuState | null>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  const [viewMode, setViewMode] = useState<ViewMode>(() =>
    (localStorage.getItem('ferrico:viewMode') as ViewMode) ?? 'list'
  )
  const [sortKey, setSortKey] = useState<SortKey>(() =>
    (localStorage.getItem('ferrico:sortKey') as SortKey) ?? 'date-desc'
  )

  useEffect(() => { localStorage.setItem('ferrico:viewMode', viewMode) }, [viewMode])
  useEffect(() => { localStorage.setItem('ferrico:sortKey', sortKey) }, [sortKey])

  useEffect(() => {
    const t = setTimeout(() => setSearch(searchInput), 300)
    return () => clearTimeout(t)
  }, [searchInput])

  const loadAll = useCallback(async () => {
    try {
      const isBin = selection.type === 'bin'
      const [b, f, t, count, inbox, bCount] = await Promise.all([
        isBin
          ? invoke<Bookmark[]>('get_bin_bookmarks')
          : invoke<Bookmark[]>('get_bookmarks', {
              folderId: selection.type === 'folder' ? selection.id : null,
              tagId: selection.type === 'tag' ? selection.id : null,
              search: search || null,
              inboxOnly: selection.type === 'inbox',
            }),
        invoke<Folder[]>('get_folders'),
        invoke<Tag[]>('get_tags'),
        invoke<number>('get_bookmark_count'),
        invoke<number>('get_inbox_count'),
        invoke<number>('get_bin_count'),
      ])
      setBookmarks(b)
      setFolders(f)
      setTags(t)
      setTotalCount(count)
      setInboxCount(inbox)
      setBinCount(bCount)
      setError(null)
    } catch (e) {
      setError(extractErrorMessage(e))
      // Ensure we exit the loading state even on error
      setBookmarks((prev) => prev ?? [])
    }
  }, [selection, search])

  useEffect(() => { loadAll() }, [loadAll])

  // Purge bin items older than 30 days on startup
  useEffect(() => {
    invoke('purge_expired_bin', { days: 30 }).catch(() => {})
  }, [])

  // Reload when browser extension adds a bookmark via the HTTP API
  useEffect(() => {
    let active = true
    let unlisten: UnlistenFn | undefined
    subscribeToBookmarkAdded(loadAll)
      .then((fn) => {
        if (active) unlisten = fn
        else fn()
      })
      .catch((e) => console.error('[ferrico] bookmark-added listener failed:', e))
    return () => {
      active = false
      unlisten?.()
    }
  }, [loadAll])

  // Global keyboard shortcuts
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const mod = e.metaKey || e.ctrlKey
      if (modal) return
      if (mod && e.key === 'n') { e.preventDefault(); setModal('add-bookmark') }
      if (mod && e.key === 'f') { e.preventDefault(); searchRef.current?.focus() }
      if (mod && e.key === ',') { e.preventDefault(); setModal('settings') }
      if (e.key === 'Escape' && searchInput) { setSearchInput('') }
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [modal, searchInput])

  const sortedBookmarks = useMemo(() => {
    if (!bookmarks) return null
    const arr = [...bookmarks]
    switch (sortKey) {
      case 'date-desc': return arr.sort((a, b) => b.created_at - a.created_at)
      case 'date-asc':  return arr.sort((a, b) => a.created_at - b.created_at)
      case 'title-asc': return arr.sort((a, b) => a.title.localeCompare(b.title))
      case 'title-desc': return arr.sort((a, b) => b.title.localeCompare(a.title))
      case 'domain-asc': return arr.sort((a, b) => domainOf(a.url).localeCompare(domainOf(b.url)))
      default: return arr
    }
  }, [bookmarks, sortKey])

  const handleAddBookmark = useCallback(async (data: {
    url: string; title: string; description: string
    folder_id: string | null; tag_ids: string[]; feed_url: string | null
  }) => {
    try {
      await invoke('add_bookmark', { input: { ...data, favicon_url: duckduckgoFavicon(data.url) || null } })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

  const handleDeleteBookmark = useCallback(async (id: string) => {
    try {
      await invoke('delete_bookmark', { id })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

  const handleAddFolder = useCallback(async (name: string) => {
    try {
      await invoke('add_folder', { name, parentId: null })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

  const handleDeleteFolder = useCallback(async (id: string) => {
    try {
      await invoke('delete_folder', { id })
      if (selection.type === 'folder' && selection.id === id) setSelection({ type: 'all' })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll, selection])

  const handleAddTag = useCallback(async (name: string, color: string) => {
    try {
      await invoke('add_tag', { name, color })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

  const handleDeleteTag = useCallback(async (id: string) => {
    try {
      await invoke('delete_tag', { id })
      if (selection.type === 'tag' && selection.id === id) setSelection({ type: 'all' })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll, selection])

  const handleRestoreBookmark = useCallback(async (id: string) => {
    try {
      await invoke('restore_bookmark', { id })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

  const handleDeleteBookmarkForever = useCallback(async (id: string) => {
    try {
      await invoke('permanently_delete_bookmark', { id })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

  const handleEmptyBin = useCallback(async () => {
    try {
      await invoke('empty_bin')
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

  // Ref-based fallback for WKWebView where getData() can return empty
  const draggingIdRef = useRef<string | null>(null)

  const handleBookmarkDragStart = useCallback((e: React.DragEvent, bookmark: Bookmark) => {
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('text/plain', bookmark.id)
    draggingIdRef.current = bookmark.id
  }, [])

  const handleDropBookmark = useCallback(async (bookmarkId: string, folderId: string | null) => {
    const id = bookmarkId || draggingIdRef.current
    draggingIdRef.current = null
    if (!id) return
    try {
      await invoke('move_bookmark', { id, folderId })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [loadAll])

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
    setCtxMenu({ x: e.clientX, y: e.clientY, items: [{ label: 'Delete Folder', danger: true, action: () => handleDeleteFolder(folder.id) }] })
  }, [handleDeleteFolder])

  const openTagContext = useCallback((e: React.MouseEvent, tag: Tag) => {
    e.preventDefault()
    setCtxMenu({ x: e.clientX, y: e.clientY, items: [{ label: 'Delete Tag', danger: true, action: () => handleDeleteTag(tag.id) }] })
  }, [handleDeleteTag])

  function selectionTitle(): string {
    if (selection.type === 'all') return 'All Bookmarks'
    if (selection.type === 'inbox') return 'Inbox'
    if (selection.type === 'bin') return 'Bin'
    if (selection.type === 'folder') return folders.find((f) => f.id === selection.id)?.name ?? 'Folder'
    return tags.find((t) => t.id === selection.id)?.name ?? 'Tag'
  }

  const loading = sortedBookmarks === null
  const hasBookmarks = !loading && sortedBookmarks.length > 0
  const isBinView = selection.type === 'bin'

  return (
    <div className="flex h-screen overflow-hidden" style={{ background: 'var(--bg-base)', color: 'var(--text-primary)' }}>
      <Sidebar
        folders={folders}
        tags={tags}
        selection={selection}
        bookmarkCount={totalCount}
        inboxCount={inboxCount}
        binCount={binCount}
        onSelect={setSelection}
        onAddFolder={() => setModal('add-folder')}
        onDeleteFolder={handleDeleteFolder}
        onAddTag={() => setModal('add-tag')}
        onDeleteTag={handleDeleteTag}
        onOpenSettings={() => setModal('settings')}
        onFolderContext={openFolderContext}
        onTagContext={openTagContext}
        onDropBookmark={handleDropBookmark}
      />

      <div className="flex-1 flex flex-col min-w-0 overflow-hidden">
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
          className="flex items-center gap-3 px-6 py-3.5 flex-none"
          style={{ borderBottom: '1px solid var(--border-dim)', background: 'var(--bg-base)' }}
        >
          <h1 className="font-semibold text-sm flex-none" style={{ color: 'var(--text-primary)' }}>
            {selectionTitle()}
          </h1>

          <div className="flex-1" />

          <SortDropdown value={sortKey} onChange={setSortKey} />

          <ViewToggle value={viewMode} onChange={setViewMode} />

          <div
            className="flex items-center gap-2 px-3 py-2 rounded-lg transition-all duration-200 w-56"
            style={{
              background: 'var(--bg-elevated)',
              border: `1px solid ${searchFocused ? 'var(--border-bright)' : 'var(--border-dim)'}`,
              boxShadow: searchFocused ? '0 0 0 2px var(--accent-glow)' : 'none',
            }}
          >
            <span className="flex-none" style={{ color: searchFocused ? 'var(--accent)' : 'var(--text-muted)' }} aria-hidden="true">
              <IconSearch size={13} />
            </span>
            <input
              ref={searchRef}
              type="search"
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              onFocus={() => setSearchFocused(true)}
              onBlur={() => setSearchFocused(false)}
              placeholder="Search…"
              aria-label="Search bookmarks"
              className="bg-transparent text-sm flex-1 min-w-0 outline-none"
              style={{ color: 'var(--text-primary)' }}
            />
            {searchInput && (
              <button
                onClick={() => setSearchInput('')}
                className="flex-none transition-colors duration-150 cursor-pointer"
                style={{ color: 'var(--text-muted)' }}
                onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-secondary)')}
                onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
                aria-label="Clear search"
              >
                <IconClose size={11} />
              </button>
            )}
          </div>

          {isBinView ? (
            binCount > 0 && (
              <button
                onClick={handleEmptyBin}
                className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors duration-150 flex-none cursor-pointer"
                style={{ border: '1px solid var(--border-mid)', color: 'var(--red)' }}
                onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--red)')}
                onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
                aria-label="Empty bin"
              >
                Empty Bin
              </button>
            )
          ) : (
            <>
              {selection.type === 'inbox' && (bookmarks?.length ?? 0) > 0 && (
                <button
                  onClick={() => setModal('inbox-sort')}
                  className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors duration-150 flex-none cursor-pointer"
                  style={{ border: '1px solid var(--accent)', color: 'var(--accent)' }}
                  onMouseEnter={(e) => (e.currentTarget.style.background = 'rgba(200,160,90,0.08)')}
                  onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
                  aria-label="Sort inbox with AI"
                >
                  <IconSparkles />
                  AI Sort
                </button>
              )}

              <button
                onClick={() => setModal('import-csv')}
                className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm font-medium transition-colors duration-150 flex-none cursor-pointer"
                style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
                onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
                onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
                aria-label="Import CSV"
              >
                Import CSV
              </button>

              <button
                onClick={() => setModal('add-bookmark')}
                onMouseEnter={() => setAddHovered(true)}
                onMouseLeave={() => setAddHovered(false)}
                className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 flex-none cursor-pointer"
                style={{ background: 'var(--accent)', color: '#0c0b0a', opacity: addHovered ? 0.88 : 1 }}
                aria-label="Add bookmark"
                aria-keyshortcuts="Control+N Meta+N"
              >
                <IconPlus size={13} />
                Add
              </button>
            </>
          )}
        </header>

        {/* Column headers — list mode only */}
        {hasBookmarks && viewMode === 'list' && (
          <div
            className="flex items-center gap-4 px-6 py-2 flex-none"
            style={{ borderBottom: '1px solid var(--border-dim)' }}
            aria-hidden="true"
          >
            <div className="w-7 flex-none" />
            <span className="flex-1 text-xs uppercase tracking-widest font-medium" style={{ color: 'var(--text-muted)' }}>Title</span>
            <span className="hidden lg:block text-xs uppercase tracking-widest font-medium w-28 flex-none" style={{ color: 'var(--text-muted)' }}>Tags</span>
            <span className="hidden md:block text-xs uppercase tracking-widest font-medium w-20 text-right flex-none" style={{ color: 'var(--text-muted)' }}>
              {isBinView ? 'Deleted' : 'Added'}
            </span>
            <div className={isBinView ? 'w-9 flex-none' : 'w-5 flex-none'} />
          </div>
        )}

        {/* Main content — flex-1 + min-h-0 gives the list/grid a bounded, scrollable height */}
        <main className="flex-1 min-h-0">
          {loading ? (
            <LoadingSkeleton />
          ) : sortedBookmarks.length === 0 ? (
            isBinView
              ? <div className="flex items-center justify-center h-full" style={{ color: 'var(--text-muted)' }}>
                  <p className="text-sm">Bin is empty</p>
                </div>
              : <EmptyState onAdd={() => setModal('add-bookmark')} />
          ) : viewMode === 'grid' ? (
            <BookmarkGrid
              bookmarks={sortedBookmarks}
              onDelete={isBinView ? handleDeleteBookmarkForever : handleDeleteBookmark}
              onContext={isBinView ? openBinBookmarkContext : openBookmarkContext}
              onDragStart={!isBinView ? handleBookmarkDragStart : undefined}
            />
          ) : (
            <BookmarkList
              bookmarks={sortedBookmarks}
              onDelete={isBinView ? handleDeleteBookmarkForever : handleDeleteBookmark}
              onContext={isBinView ? openBinBookmarkContext : openBookmarkContext}
              isBinView={isBinView}
              onRestore={handleRestoreBookmark}
              onDragStart={!isBinView ? handleBookmarkDragStart : undefined}
            />
          )}
        </main>
      </div>

      {modal === 'add-bookmark' && (
        <AddBookmarkModal folders={folders} tags={tags} onAdd={handleAddBookmark} onClose={() => setModal(null)} />
      )}
      {modal === 'add-folder' && (
        <AddFolderModal onAdd={handleAddFolder} onClose={() => setModal(null)} />
      )}
      {modal === 'add-tag' && (
        <AddTagModal onAdd={handleAddTag} onClose={() => setModal(null)} />
      )}
      {modal === 'settings' && (
        <SettingsModal onClose={() => setModal(null)} onClear={() => { setModal(null); loadAll() }} />
      )}
      {modal === 'import-csv' && (
        <ImportCsvModal onClose={() => setModal(null)} onDone={loadAll} />
      )}
      {modal === 'inbox-sort' && bookmarks && (
        <InboxSortModal
          bookmarks={bookmarks}
          folders={folders}
          onClose={() => setModal(null)}
          onDone={loadAll}
        />
      )}

      {ctxMenu && <ContextMenu state={ctxMenu} onClose={() => setCtxMenu(null)} />}
    </div>
  )
}
