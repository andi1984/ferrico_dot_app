import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { subscribeToBookmarkAdded, type UnlistenFn } from './events'
import type { Bookmark, Folder, Tag, Selection } from './types'
import { extractErrorMessage, duckduckgoFavicon } from './utils'
import { ContextMenu, type CtxMenuState } from './components/ContextMenu'
import { BookmarkRow } from './components/BookmarkRow'
import { AddBookmarkModal } from './components/AddBookmarkModal'
import { AddFolderModal } from './components/AddFolderModal'
import { AddTagModal } from './components/AddTagModal'
import { SettingsModal } from './components/SettingsModal'
import { Sidebar } from './components/Sidebar'
import { EmptyState } from './components/EmptyState'
import { IconClose, IconPlus, IconSearch } from './components/icons'

type Modal = 'add-bookmark' | 'add-folder' | 'add-tag' | 'settings' | null

export default function App() {
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([])
  const [folders, setFolders] = useState<Folder[]>([])
  const [tags, setTags] = useState<Tag[]>([])
  const [selection, setSelection] = useState<Selection>({ type: 'all' })
  const [searchInput, setSearchInput] = useState('')
  const [search, setSearch] = useState('')
  const [totalCount, setTotalCount] = useState(0)
  const [modal, setModal] = useState<Modal>(null)
  const [error, setError] = useState<string | null>(null)
  const [addHovered, setAddHovered] = useState(false)
  const [searchFocused, setSearchFocused] = useState(false)
  const [ctxMenu, setCtxMenu] = useState<CtxMenuState | null>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const t = setTimeout(() => setSearch(searchInput), 300)
    return () => clearTimeout(t)
  }, [searchInput])

  const loadAll = useCallback(async () => {
    try {
      const [b, f, t, count] = await Promise.all([
        invoke<Bookmark[]>('get_bookmarks', {
          folderId: selection.type === 'folder' ? selection.id : null,
          tagId: selection.type === 'tag' ? selection.id : null,
          search: search || null,
        }),
        invoke<Folder[]>('get_folders'),
        invoke<Tag[]>('get_tags'),
        invoke<number>('get_bookmark_count'),
      ])
      setBookmarks(b)
      setFolders(f)
      setTags(t)
      setTotalCount(count)
      setError(null)
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }, [selection, search])

  useEffect(() => { loadAll() }, [loadAll])

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

  async function handleAddBookmark(data: { url: string; title: string; description: string; folder_id: string | null; tag_ids: string[]; feed_url: string | null }) {
    try {
      await invoke('add_bookmark', { input: { ...data, favicon_url: duckduckgoFavicon(data.url) || null } })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }

  async function handleDeleteBookmark(id: string) {
    try {
      await invoke('delete_bookmark', { id })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }

  async function handleAddFolder(name: string) {
    try {
      await invoke('add_folder', { name, parentId: null })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }

  async function handleDeleteFolder(id: string) {
    try {
      await invoke('delete_folder', { id })
      if (selection.type === 'folder' && selection.id === id) setSelection({ type: 'all' })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }

  async function handleAddTag(name: string, color: string) {
    try {
      await invoke('add_tag', { name, color })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }

  async function handleDeleteTag(id: string) {
    try {
      await invoke('delete_tag', { id })
      if (selection.type === 'tag' && selection.id === id) setSelection({ type: 'all' })
      loadAll()
    } catch (e) {
      setError(extractErrorMessage(e))
    }
  }

  function selectionTitle(): string {
    if (selection.type === 'all') return 'All Bookmarks'
    if (selection.type === 'folder') return folders.find((f) => f.id === selection.id)?.name ?? 'Folder'
    return tags.find((t) => t.id === selection.id)?.name ?? 'Tag'
  }

  function openBookmarkContext(e: React.MouseEvent, bookmark: Bookmark) {
    e.preventDefault()
    setCtxMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: 'Open in Browser', action: () => invoke('open_url', { url: bookmark.url }).catch(() => {}) },
        { label: 'Copy URL', action: () => navigator.clipboard.writeText(bookmark.url) },
        { label: 'Copy Title', action: () => navigator.clipboard.writeText(bookmark.title) },
        { sep: true, label: '', action: () => {} },
        { label: 'Delete', danger: true, action: () => handleDeleteBookmark(bookmark.id) },
      ],
    })
  }

  function openFolderContext(e: React.MouseEvent, folder: Folder) {
    e.preventDefault()
    setCtxMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: 'Delete Folder', danger: true, action: () => handleDeleteFolder(folder.id) },
      ],
    })
  }

  function openTagContext(e: React.MouseEvent, tag: Tag) {
    e.preventDefault()
    setCtxMenu({
      x: e.clientX,
      y: e.clientY,
      items: [
        { label: 'Delete Tag', danger: true, action: () => handleDeleteTag(tag.id) },
      ],
    })
  }

  return (
    <div className="flex h-screen overflow-hidden" style={{ background: 'var(--bg-base)', color: 'var(--text-primary)' }}>
      <Sidebar
        folders={folders}
        tags={tags}
        selection={selection}
        bookmarkCount={totalCount}
        onSelect={setSelection}
        onAddFolder={() => setModal('add-folder')}
        onDeleteFolder={handleDeleteFolder}
        onAddTag={() => setModal('add-tag')}
        onDeleteTag={handleDeleteTag}
        onOpenSettings={() => setModal('settings')}
        onFolderContext={openFolderContext}
        onTagContext={openTagContext}
      />

      <div className="flex-1 flex flex-col min-w-0">
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
          className="flex items-center gap-4 px-6 py-3.5 flex-none"
          style={{ borderBottom: '1px solid var(--border-dim)', background: 'var(--bg-base)' }}
        >
          <h1 className="font-semibold text-sm flex-none" style={{ color: 'var(--text-primary)' }}>
            {selectionTitle()}
          </h1>

          <div className="flex-1" />

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
        </header>

        {bookmarks.length > 0 && (
          <div
            className="flex items-center gap-4 px-6 py-2 flex-none"
            style={{ borderBottom: '1px solid var(--border-dim)' }}
            aria-hidden="true"
          >
            <div className="w-7 flex-none" />
            <span className="flex-1 text-xs uppercase tracking-widest font-medium" style={{ color: 'var(--text-muted)' }}>Title</span>
            <span className="hidden lg:block text-xs uppercase tracking-widest font-medium w-28 flex-none" style={{ color: 'var(--text-muted)' }}>Tags</span>
            <span className="hidden md:block text-xs uppercase tracking-widest font-medium w-20 text-right flex-none" style={{ color: 'var(--text-muted)' }}>Added</span>
            <div className="w-5 flex-none" />
          </div>
        )}

        <main className="flex-1 overflow-y-auto">
          {bookmarks.length === 0 ? (
            <EmptyState onAdd={() => setModal('add-bookmark')} />
          ) : (
            <div>
              {bookmarks.map((b, i) => (
                <BookmarkRow
                  key={b.id}
                  bookmark={b}
                  onDelete={handleDeleteBookmark}
                  onContext={openBookmarkContext}
                  index={i}
                />
              ))}
            </div>
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
        <SettingsModal onClose={() => setModal(null)} />
      )}

      {ctxMenu && <ContextMenu state={ctxMenu} onClose={() => setCtxMenu(null)} />}
    </div>
  )
}
