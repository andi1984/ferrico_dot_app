import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark, Folder, Tag, Selection } from './types'

// ─── Utils ───────────────────────────────────────────────────────────────────

function duckduckgoFavicon(url: string): string {
  try {
    const { hostname } = new URL(url)
    return `https://icons.duckduckgo.com/ip3/${hostname}.ico`
  } catch {
    return ''
  }
}

function domainOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

function initials(title: string): string {
  return title.trim().charAt(0).toUpperCase() || '?'
}

// ─── Favicon ─────────────────────────────────────────────────────────────────

function Favicon({ storedUrl, bookmarkUrl, title }: { storedUrl: string | null; bookmarkUrl: string; title: string }) {
  const [failed, setFailed] = useState(false)
  const src = storedUrl || duckduckgoFavicon(bookmarkUrl)

  if (!src || failed) {
    return (
      <div className="w-8 h-8 rounded-lg bg-indigo-500/20 flex items-center justify-center text-indigo-400 font-semibold text-sm flex-none">
        {initials(title)}
      </div>
    )
  }

  return (
    <img
      src={src}
      alt=""
      className="w-8 h-8 rounded-lg object-contain flex-none"
      onError={() => setFailed(true)}
    />
  )
}

// ─── BookmarkCard ─────────────────────────────────────────────────────────────

function BookmarkCard({
  bookmark,
  onDelete,
}: {
  bookmark: Bookmark
  onDelete: (id: string) => void
}) {
  return (
    <div className="group bg-slate-800 hover:bg-slate-750 border border-slate-700 hover:border-slate-600 rounded-xl p-4 flex flex-col gap-3 transition-all duration-150">
      <div className="flex items-start gap-3">
        <Favicon storedUrl={bookmark.favicon_url} bookmarkUrl={bookmark.url} title={bookmark.title} />
        <div className="flex-1 min-w-0">
          <a
            href={bookmark.url}
            target="_blank"
            rel="noreferrer"
            className="text-slate-100 font-medium text-sm leading-snug hover:text-indigo-400 transition-colors line-clamp-2 block"
          >
            {bookmark.title}
          </a>
          <p className="text-slate-500 text-xs mt-0.5 truncate">{domainOf(bookmark.url)}</p>
        </div>
        <button
          onClick={() => onDelete(bookmark.id)}
          className="opacity-0 group-hover:opacity-100 text-slate-600 hover:text-red-400 transition-all p-1 -mt-1 -mr-1 rounded"
          title="Delete"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      {bookmark.description && (
        <p className="text-slate-400 text-xs leading-relaxed line-clamp-2">{bookmark.description}</p>
      )}

      {bookmark.tags.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {bookmark.tags.map((tag) => (
            <span
              key={tag.id}
              className="px-2 py-0.5 rounded-full text-xs font-medium"
              style={{ backgroundColor: tag.color + '22', color: tag.color }}
            >
              {tag.name}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

// ─── Add Bookmark Modal ───────────────────────────────────────────────────────

interface AddBookmarkModalProps {
  folders: Folder[]
  tags: Tag[]
  onAdd: (data: {
    url: string
    title: string
    description: string
    folder_id: string | null
    tag_ids: string[]
    feed_url: string | null
  }) => void
  onClose: () => void
}

function AddBookmarkModal({ folders, tags, onAdd, onClose }: AddBookmarkModalProps) {
  const [url, setUrl] = useState('')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [folderId, setFolderId] = useState<string>('')
  const [selectedTags, setSelectedTags] = useState<Set<string>>(new Set())
  const urlRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    urlRef.current?.focus()
  }, [])

  function toggleTag(id: string) {
    setSelectedTags((prev) => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!url.trim() || !title.trim()) return
    onAdd({
      url: url.trim(),
      title: title.trim(),
      description: description.trim(),
      folder_id: folderId || null,
      tag_ids: [...selectedTags],
      feed_url: null,
    })
  }

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-slate-800 border border-slate-700 rounded-2xl w-full max-w-md shadow-2xl">
        <div className="flex items-center justify-between px-6 pt-6 pb-4 border-b border-slate-700">
          <h2 className="text-slate-100 font-semibold text-base">Add Bookmark</h2>
          <button onClick={onClose} className="text-slate-500 hover:text-slate-300 transition-colors">
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <form onSubmit={handleSubmit} className="p-6 flex flex-col gap-4">
          <div>
            <label className="text-slate-400 text-xs font-medium uppercase tracking-wide block mb-1.5">URL *</label>
            <input
              ref={urlRef}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://example.com"
              className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm placeholder-slate-600 focus:outline-none focus:border-indigo-500 transition-colors"
            />
          </div>

          <div>
            <label className="text-slate-400 text-xs font-medium uppercase tracking-wide block mb-1.5">Title *</label>
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Page title"
              className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm placeholder-slate-600 focus:outline-none focus:border-indigo-500 transition-colors"
            />
          </div>

          <div>
            <label className="text-slate-400 text-xs font-medium uppercase tracking-wide block mb-1.5">Description</label>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Optional note..."
              rows={2}
              className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm placeholder-slate-600 focus:outline-none focus:border-indigo-500 transition-colors resize-none"
            />
          </div>

          {folders.length > 0 && (
            <div>
              <label className="text-slate-400 text-xs font-medium uppercase tracking-wide block mb-1.5">Folder</label>
              <select
                value={folderId}
                onChange={(e) => setFolderId(e.target.value)}
                className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm focus:outline-none focus:border-indigo-500 transition-colors"
              >
                <option value="">No folder</option>
                {folders.map((f) => (
                  <option key={f.id} value={f.id}>{f.name}</option>
                ))}
              </select>
            </div>
          )}

          {tags.length > 0 && (
            <div>
              <label className="text-slate-400 text-xs font-medium uppercase tracking-wide block mb-1.5">Tags</label>
              <div className="flex flex-wrap gap-2">
                {tags.map((tag) => (
                  <button
                    key={tag.id}
                    type="button"
                    onClick={() => toggleTag(tag.id)}
                    className="px-2.5 py-1 rounded-full text-xs font-medium border transition-all"
                    style={
                      selectedTags.has(tag.id)
                        ? { backgroundColor: tag.color + '33', color: tag.color, borderColor: tag.color }
                        : { backgroundColor: 'transparent', color: '#64748b', borderColor: '#334155' }
                    }
                  >
                    {tag.name}
                  </button>
                ))}
              </div>
            </div>
          )}

          <div className="flex gap-3 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="flex-1 px-4 py-2 rounded-lg border border-slate-700 text-slate-400 text-sm hover:text-slate-300 hover:border-slate-600 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              className="flex-1 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors"
            >
              Save
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

// ─── Add Folder Modal ─────────────────────────────────────────────────────────

function AddFolderModal({ onAdd, onClose }: { onAdd: (name: string) => void; onClose: () => void }) {
  const [name, setName] = useState('')
  const ref = useRef<HTMLInputElement>(null)

  useEffect(() => { ref.current?.focus() }, [])

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    onAdd(name.trim())
  }

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-slate-800 border border-slate-700 rounded-2xl w-full max-w-sm shadow-2xl p-6">
        <h2 className="text-slate-100 font-semibold text-base mb-4">New Folder</h2>
        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <input
            ref={ref}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Folder name"
            className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm placeholder-slate-600 focus:outline-none focus:border-indigo-500 transition-colors"
          />
          <div className="flex gap-3">
            <button type="button" onClick={onClose} className="flex-1 px-4 py-2 rounded-lg border border-slate-700 text-slate-400 text-sm hover:text-slate-300 transition-colors">
              Cancel
            </button>
            <button type="submit" className="flex-1 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors">
              Create
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

// ─── Add Tag Modal ────────────────────────────────────────────────────────────

const TAG_COLORS = ['#6366f1', '#ec4899', '#f59e0b', '#10b981', '#3b82f6', '#ef4444', '#8b5cf6', '#14b8a6']

function AddTagModal({ onAdd, onClose }: { onAdd: (name: string, color: string) => void; onClose: () => void }) {
  const [name, setName] = useState('')
  const [color, setColor] = useState(TAG_COLORS[0])
  const ref = useRef<HTMLInputElement>(null)

  useEffect(() => { ref.current?.focus() }, [])

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    onAdd(name.trim(), color)
  }

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-slate-800 border border-slate-700 rounded-2xl w-full max-w-sm shadow-2xl p-6">
        <h2 className="text-slate-100 font-semibold text-base mb-4">New Tag</h2>
        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <input
            ref={ref}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Tag name"
            className="w-full bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-100 text-sm placeholder-slate-600 focus:outline-none focus:border-indigo-500 transition-colors"
          />
          <div className="flex gap-2 flex-wrap">
            {TAG_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                onClick={() => setColor(c)}
                className="w-7 h-7 rounded-full transition-transform"
                style={{
                  backgroundColor: c,
                  outline: color === c ? `3px solid ${c}` : 'none',
                  outlineOffset: '2px',
                  transform: color === c ? 'scale(1.15)' : 'scale(1)',
                }}
              />
            ))}
          </div>
          <div className="flex gap-3">
            <button type="button" onClick={onClose} className="flex-1 px-4 py-2 rounded-lg border border-slate-700 text-slate-400 text-sm hover:text-slate-300 transition-colors">
              Cancel
            </button>
            <button type="submit" className="flex-1 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors">
              Create
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

// ─── Settings Modal ───────────────────────────────────────────────────────────

function SettingsModal({ onClose }: { onClose: () => void }) {
  const [token, setToken] = useState('')
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    invoke<string>('get_api_token').then(setToken)
  }, [])

  function copy() {
    navigator.clipboard.writeText(token)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  async function handleExport() {
    const opml = await invoke<string>('export_opml')
    const blob = new Blob([opml], { type: 'text/xml' })
    const objectUrl = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = objectUrl
    a.download = 'ferrico-bookmarks.opml'
    a.click()
    URL.revokeObjectURL(objectUrl)
  }

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-slate-800 border border-slate-700 rounded-2xl w-full max-w-md shadow-2xl">
        <div className="flex items-center justify-between px-6 pt-6 pb-4 border-b border-slate-700">
          <h2 className="text-slate-100 font-semibold text-base">Settings</h2>
          <button onClick={onClose} className="text-slate-500 hover:text-slate-300 transition-colors">
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="p-6 flex flex-col gap-6">
          <div>
            <p className="text-slate-100 text-sm font-medium mb-1">Browser Extension API Token</p>
            <p className="text-slate-500 text-xs mb-3">Paste this token into the Ferrico extension options page to link it to this app.</p>
            <div className="flex gap-2">
              <code className="flex-1 bg-slate-900 border border-slate-700 rounded-lg px-3 py-2 text-slate-300 text-xs font-mono truncate">
                {token || '…'}
              </code>
              <button
                onClick={copy}
                className="px-3 py-2 rounded-lg bg-slate-700 hover:bg-slate-600 text-slate-300 text-xs font-medium transition-colors whitespace-nowrap"
              >
                {copied ? 'Copied!' : 'Copy'}
              </button>
            </div>
          </div>

          <div className="border-t border-slate-700 pt-6">
            <p className="text-slate-100 text-sm font-medium mb-1">Export Bookmarks</p>
            <p className="text-slate-500 text-xs mb-3">Download all bookmarks as an OPML file.</p>
            <button
              onClick={handleExport}
              className="px-4 py-2 rounded-lg bg-slate-700 hover:bg-slate-600 text-slate-200 text-sm transition-colors"
            >
              Export OPML
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

// ─── Sidebar ─────────────────────────────────────────────────────────────────

interface SidebarProps {
  folders: Folder[]
  tags: Tag[]
  selection: Selection
  bookmarkCount: number
  onSelect: (s: Selection) => void
  onAddFolder: () => void
  onDeleteFolder: (id: string) => void
  onAddTag: () => void
  onDeleteTag: (id: string) => void
  onOpenSettings: () => void
}

function Sidebar({
  folders, tags, selection, bookmarkCount,
  onSelect, onAddFolder, onDeleteFolder, onAddTag, onDeleteTag, onOpenSettings,
}: SidebarProps) {
  function isSelected(s: Selection): boolean {
    if (s.type !== selection.type) return false
    if (s.type === 'all') return true
    if (s.type === 'folder' && selection.type === 'folder') return s.id === selection.id
    if (s.type === 'tag' && selection.type === 'tag') return s.id === selection.id
    return false
  }

  const itemClass = (active: boolean) =>
    `flex items-center gap-2.5 w-full px-3 py-1.5 rounded-lg text-sm transition-colors text-left group ${
      active
        ? 'bg-indigo-500/15 text-indigo-300'
        : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800'
    }`

  return (
    <aside className="w-56 flex-none bg-slate-900 border-r border-slate-800 flex flex-col h-full overflow-y-auto">
      {/* Logo */}
      <div className="px-4 py-5 border-b border-slate-800">
        <div className="flex items-center gap-2.5">
          <div className="w-7 h-7 rounded-lg bg-indigo-600 flex items-center justify-center">
            <svg className="w-4 h-4 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M17.593 3.322c1.1.128 1.907 1.077 1.907 2.185V21L12 17.25 4.5 21V5.507c0-1.108.806-2.057 1.907-2.185a48.507 48.507 0 0111.186 0z" />
            </svg>
          </div>
          <span className="text-slate-100 font-semibold text-base tracking-tight">Ferrico</span>
        </div>
      </div>

      <nav className="flex-1 px-2 py-3 flex flex-col gap-0.5">
        {/* All */}
        <button
          onClick={() => onSelect({ type: 'all' })}
          className={itemClass(isSelected({ type: 'all' }))}
        >
          <svg className="w-4 h-4 flex-none" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M3.75 12h16.5m-16.5 3.75h16.5M3.75 19.5h16.5M5.625 4.5h12.75a1.875 1.875 0 010 3.75H5.625a1.875 1.875 0 010-3.75z" />
          </svg>
          <span className="flex-1">All Bookmarks</span>
          <span className="text-xs text-slate-600">{bookmarkCount}</span>
        </button>

        {/* Folders */}
        <div className="mt-4 mb-1 px-3 flex items-center justify-between">
          <span className="text-xs font-semibold uppercase tracking-wider text-slate-600">Folders</span>
          <button
            onClick={onAddFolder}
            className="text-slate-600 hover:text-slate-400 transition-colors"
            title="New folder"
          >
            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
          </button>
        </div>

        {folders.map((folder) => (
          <button
            key={folder.id}
            onClick={() => onSelect({ type: 'folder', id: folder.id })}
            className={itemClass(isSelected({ type: 'folder', id: folder.id }))}
          >
            <svg className="w-4 h-4 flex-none" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M2.25 12.75V12A2.25 2.25 0 014.5 9.75h15A2.25 2.25 0 0121.75 12v.75m-8.69-6.44l-2.12-2.12a1.5 1.5 0 00-1.061-.44H4.5A2.25 2.25 0 002.25 6v12a2.25 2.25 0 002.25 2.25h15A2.25 2.25 0 0021.75 18V9a2.25 2.25 0 00-2.25-2.25h-5.379a1.5 1.5 0 01-1.06-.44z" />
            </svg>
            <span className="flex-1 truncate">{folder.name}</span>
            <button
              onClick={(e) => { e.stopPropagation(); onDeleteFolder(folder.id) }}
              className="opacity-0 group-hover:opacity-100 text-slate-600 hover:text-red-400 transition-all"
            >
              <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </button>
        ))}

        {folders.length === 0 && (
          <p className="px-3 py-1 text-xs text-slate-700 italic">No folders yet</p>
        )}

        {/* Tags */}
        <div className="mt-4 mb-1 px-3 flex items-center justify-between">
          <span className="text-xs font-semibold uppercase tracking-wider text-slate-600">Tags</span>
          <button
            onClick={onAddTag}
            className="text-slate-600 hover:text-slate-400 transition-colors"
            title="New tag"
          >
            <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
          </button>
        </div>

        {tags.map((tag) => (
          <button
            key={tag.id}
            onClick={() => onSelect({ type: 'tag', id: tag.id })}
            className={itemClass(isSelected({ type: 'tag', id: tag.id }))}
          >
            <span className="w-2.5 h-2.5 rounded-full flex-none" style={{ backgroundColor: tag.color }} />
            <span className="flex-1 truncate">{tag.name}</span>
            <button
              onClick={(e) => { e.stopPropagation(); onDeleteTag(tag.id) }}
              className="opacity-0 group-hover:opacity-100 text-slate-600 hover:text-red-400 transition-all"
            >
              <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </button>
        ))}

        {tags.length === 0 && (
          <p className="px-3 py-1 text-xs text-slate-700 italic">No tags yet</p>
        )}
      </nav>

      {/* Settings */}
      <div className="px-2 py-3 border-t border-slate-800">
        <button
          onClick={onOpenSettings}
          className="flex items-center gap-2.5 w-full px-3 py-1.5 rounded-lg text-sm text-slate-500 hover:text-slate-300 hover:bg-slate-800 transition-colors"
        >
          <svg className="w-4 h-4 flex-none" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.324.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 011.37.49l1.296 2.247a1.125 1.125 0 01-.26 1.431l-1.003.827c-.293.24-.438.613-.431.992a6.759 6.759 0 010 .255c-.007.378.138.75.43.99l1.005.828c.424.35.534.954.26 1.43l-1.298 2.247a1.125 1.125 0 01-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.57 6.57 0 01-.22.128c-.331.183-.581.495-.644.869l-.213 1.28c-.09.543-.56.941-1.11.941h-2.594c-.55 0-1.02-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 01-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 01-1.369-.49l-1.297-2.247a1.125 1.125 0 01.26-1.431l1.004-.827c.292-.24.437-.613.43-.992a6.932 6.932 0 010-.255c.007-.378-.138-.75-.43-.99l-1.004-.828a1.125 1.125 0 01-.26-1.43l1.297-2.247a1.125 1.125 0 011.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.087.22-.128.332-.183.582-.495.644-.869l.214-1.281z" />
            <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
          Settings
        </button>
      </div>
    </aside>
  )
}

// ─── Empty State ──────────────────────────────────────────────────────────────

function EmptyState({ onAdd }: { onAdd: () => void }) {
  return (
    <div className="flex flex-col items-center justify-center h-full gap-4 text-center px-8">
      <div className="w-16 h-16 rounded-2xl bg-slate-800 border border-slate-700 flex items-center justify-center">
        <svg className="w-8 h-8 text-slate-600" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.5}>
          <path strokeLinecap="round" strokeLinejoin="round" d="M17.593 3.322c1.1.128 1.907 1.077 1.907 2.185V21L12 17.25 4.5 21V5.507c0-1.108.806-2.057 1.907-2.185a48.507 48.507 0 0111.186 0z" />
        </svg>
      </div>
      <div>
        <p className="text-slate-300 font-medium">No bookmarks yet</p>
        <p className="text-slate-600 text-sm mt-1">Add your first bookmark to get started.</p>
      </div>
      <button
        onClick={onAdd}
        className="px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors"
      >
        Add Bookmark
      </button>
    </div>
  )
}

// ─── App ─────────────────────────────────────────────────────────────────────

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
      setError(String(e))
    }
  }, [selection, search])

  useEffect(() => { loadAll() }, [loadAll])

  async function handleAddBookmark(data: {
    url: string; title: string; description: string
    folder_id: string | null; tag_ids: string[]; feed_url: string | null
  }) {
    try {
      await invoke('add_bookmark', {
        input: { ...data, favicon_url: duckduckgoFavicon(data.url) || null },
      })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(String(e))
    }
  }

  async function handleDeleteBookmark(id: string) {
    try {
      await invoke('delete_bookmark', { id })
      loadAll()
    } catch (e) {
      setError(String(e))
    }
  }

  async function handleAddFolder(name: string) {
    try {
      await invoke('add_folder', { name, parentId: null })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(String(e))
    }
  }

  async function handleDeleteFolder(id: string) {
    try {
      await invoke('delete_folder', { id })
      if (selection.type === 'folder' && selection.id === id) {
        setSelection({ type: 'all' })
      }
      loadAll()
    } catch (e) {
      setError(String(e))
    }
  }

  async function handleAddTag(name: string, color: string) {
    try {
      await invoke('add_tag', { name, color })
      setModal(null)
      loadAll()
    } catch (e) {
      setError(String(e))
    }
  }

  async function handleDeleteTag(id: string) {
    try {
      await invoke('delete_tag', { id })
      if (selection.type === 'tag' && selection.id === id) {
        setSelection({ type: 'all' })
      }
      loadAll()
    } catch (e) {
      setError(String(e))
    }
  }

  function selectionTitle(): string {
    if (selection.type === 'all') return 'All Bookmarks'
    if (selection.type === 'folder') {
      return folders.find((f) => f.id === selection.id)?.name ?? 'Folder'
    }
    return tags.find((t) => t.id === selection.id)?.name ?? 'Tag'
  }

  return (
    <div className="flex h-screen bg-slate-950 text-slate-100 overflow-hidden">
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
      />

      <div className="flex-1 flex flex-col min-w-0">
        {/* Error banner */}
        {error && (
          <div className="bg-red-900/50 border-b border-red-700 px-6 py-2 flex items-center justify-between gap-4 flex-none">
            <p className="text-red-300 text-sm truncate">{error}</p>
            <button onClick={() => setError(null)} className="text-red-400 hover:text-red-200 flex-none text-xs">Dismiss</button>
          </div>
        )}
        {/* Header */}
        <header className="flex items-center gap-4 px-6 py-4 border-b border-slate-800 flex-none">
          <div className="flex-1">
            <h1 className="text-slate-100 font-semibold">{selectionTitle()}</h1>
          </div>

          <div className="flex items-center gap-2 bg-slate-900 border border-slate-800 rounded-lg px-3 py-1.5 w-64">
            <svg className="w-4 h-4 text-slate-600 flex-none" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
            </svg>
            <input
              type="text"
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              placeholder="Search bookmarks…"
              className="bg-transparent text-slate-300 text-sm placeholder-slate-600 focus:outline-none flex-1 min-w-0"
            />
            {searchInput && (
              <button onClick={() => setSearchInput('')} className="text-slate-600 hover:text-slate-400 transition-colors">
                <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            )}
          </div>

          <button
            onClick={() => setModal('add-bookmark')}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium transition-colors"
          >
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
            Add
          </button>
        </header>

        {/* Bookmark grid */}
        <main className="flex-1 overflow-y-auto p-6">
          {bookmarks.length === 0 ? (
            <EmptyState onAdd={() => setModal('add-bookmark')} />
          ) : (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3">
              {bookmarks.map((b) => (
                <BookmarkCard key={b.id} bookmark={b} onDelete={handleDeleteBookmark} />
              ))}
            </div>
          )}
        </main>
      </div>

      {/* Modals */}
      {modal === 'add-bookmark' && (
        <AddBookmarkModal
          folders={folders}
          tags={tags}
          onAdd={handleAddBookmark}
          onClose={() => setModal(null)}
        />
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
    </div>
  )
}
