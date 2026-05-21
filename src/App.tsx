import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark, Folder, Tag, Selection } from './types'

// ─── Error ────────────────────────────────────────────────────────────────────

function extractErrorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message)
  return String(e)
}

// ─── Utils ────────────────────────────────────────────────────────────────────

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

function formatDate(ts: number): string {
  const d = new Date(ts * 1000)
  const now = new Date()
  const diffMs = now.getTime() - d.getTime()
  const diffDays = Math.floor(diffMs / 86400000)
  if (diffDays === 0) return 'Today'
  if (diffDays === 1) return 'Yesterday'
  if (diffDays < 7) return `${diffDays}d ago`
  return d.toLocaleDateString('en', { month: 'short', day: 'numeric' })
}

// ─── Icons ────────────────────────────────────────────────────────────────────

const IconClose = ({ size = 16 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
    <path d="M18 6 6 18M6 6l12 12" />
  </svg>
)

const IconPlus = ({ size = 16 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 5v14M5 12h14" />
  </svg>
)

const IconSearch = ({ size = 15 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
    <circle cx="11" cy="11" r="8" /><path d="m21 21-4.35-4.35" />
  </svg>
)

const IconFolder = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round">
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
  </svg>
)

const IconAll = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round">
    <path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01" />
  </svg>
)

const IconSettings = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
)

const IconChevronDown = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
    <path d="m6 9 6 6 6-6" />
  </svg>
)

const IconExport = ({ size = 14 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" />
  </svg>
)

// ─── Favicon ──────────────────────────────────────────────────────────────────

function Favicon({ storedUrl, bookmarkUrl, title }: { storedUrl: string | null; bookmarkUrl: string; title: string }) {
  const [failed, setFailed] = useState(false)
  const src = storedUrl || duckduckgoFavicon(bookmarkUrl)

  if (!src || failed) {
    return (
      <div
        className="w-7 h-7 rounded flex-none flex items-center justify-center text-xs font-semibold select-none"
        style={{ background: 'var(--accent-dim)', color: 'var(--accent)' }}
      >
        {initials(title)}
      </div>
    )
  }

  return (
    <img
      src={src}
      alt=""
      className="w-7 h-7 rounded object-contain flex-none"
      style={{ background: 'var(--bg-elevated)' }}
      onError={() => setFailed(true)}
    />
  )
}

// ─── BookmarkRow ──────────────────────────────────────────────────────────────

function BookmarkRow({ bookmark, onDelete, index }: {
  bookmark: Bookmark
  onDelete: (id: string) => void
  index: number
}) {
  const [hovered, setHovered] = useState(false)
  const [titleHovered, setTitleHovered] = useState(false)

  return (
    <div
      className="anim-fade-up relative flex items-center gap-4 px-6 py-3 border-b transition-colors duration-150 cursor-default"
      style={{
        borderColor: 'var(--border-dim)',
        background: hovered ? 'var(--bg-elevated)' : 'transparent',
        animationDelay: `${Math.min(index * 22, 350)}ms`,
      }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {/* Left accent bar */}
      <div
        className="absolute left-0 top-2 bottom-2 w-0.5 rounded-full transition-opacity duration-200"
        style={{ background: 'var(--accent)', opacity: hovered ? 1 : 0 }}
      />

      <Favicon storedUrl={bookmark.favicon_url} bookmarkUrl={bookmark.url} title={bookmark.title} />

      {/* Title + domain + desc */}
      <div className="flex-1 min-w-0">
        <div className="flex items-baseline gap-2 min-w-0">
          <a
            href={bookmark.url}
            target="_blank"
            rel="noreferrer"
            className="text-sm font-medium truncate leading-snug transition-colors duration-100"
            style={{ color: titleHovered ? 'var(--accent-bright)' : 'var(--text-primary)' }}
            onMouseEnter={() => setTitleHovered(true)}
            onMouseLeave={() => setTitleHovered(false)}
          >
            {bookmark.title}
          </a>
          <span
            className="text-xs flex-none hidden sm:block"
            style={{ color: 'var(--text-muted)' }}
          >
            {domainOf(bookmark.url)}
          </span>
        </div>
        {bookmark.description && (
          <p className="text-xs mt-0.5 truncate" style={{ color: 'var(--text-secondary)' }}>
            {bookmark.description}
          </p>
        )}
      </div>

      {/* Tags */}
      {bookmark.tags.length > 0 && (
        <div className="hidden lg:flex items-center gap-1.5 flex-none">
          {bookmark.tags.slice(0, 2).map((tag) => (
            <span
              key={tag.id}
              className="px-2 py-0.5 rounded text-xs font-medium"
              style={{ background: tag.color + '1a', color: tag.color }}
            >
              {tag.name}
            </span>
          ))}
          {bookmark.tags.length > 2 && (
            <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
              +{bookmark.tags.length - 2}
            </span>
          )}
        </div>
      )}

      {/* Date */}
      <span
        className="text-xs hidden md:block flex-none w-20 text-right"
        style={{ color: 'var(--text-muted)' }}
      >
        {formatDate(bookmark.created_at)}
      </span>

      {/* Delete */}
      <button
        onClick={() => onDelete(bookmark.id)}
        className="p-1 rounded flex-none transition-all duration-150"
        style={{
          color: 'var(--text-muted)',
          opacity: hovered ? 1 : 0,
        }}
        onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--red)')}
        onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
        title="Delete bookmark"
      >
        <IconClose size={13} />
      </button>
    </div>
  )
}

// ─── Modal Shell ──────────────────────────────────────────────────────────────

function ModalShell({ title, onClose, children }: {
  title: string
  onClose: () => void
  children: React.ReactNode
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      style={{ background: 'rgba(0,0,0,0.75)', backdropFilter: 'blur(4px)' }}
    >
      <div
        className="anim-scale-in w-full max-w-md rounded-xl shadow-2xl overflow-hidden"
        style={{ background: 'var(--bg-surface)', border: '1px solid var(--border-mid)' }}
      >
        {/* Header */}
        <div
          className="flex items-center justify-between px-6 py-4"
          style={{ borderBottom: '1px solid var(--border-dim)' }}
        >
          <span className="text-sm font-semibold tracking-wide" style={{ color: 'var(--text-primary)' }}>
            {title}
          </span>
          <button
            onClick={onClose}
            className="rounded p-1 transition-colors duration-150"
            style={{ color: 'var(--text-muted)' }}
            onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-primary)')}
            onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
          >
            <IconClose size={15} />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}

// ─── Field Label ──────────────────────────────────────────────────────────────

function FieldLabel({ children }: { children: React.ReactNode }) {
  return (
    <label className="block text-xs font-medium uppercase tracking-widest mb-2" style={{ color: 'var(--text-muted)' }}>
      {children}
    </label>
  )
}

// ─── Modal Buttons ────────────────────────────────────────────────────────────

function ModalActions({ onClose, submitLabel }: { onClose: () => void; submitLabel: string }) {
  return (
    <div className="flex gap-2 pt-2">
      <button
        type="button"
        onClick={onClose}
        className="flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-colors duration-150"
        style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
        onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
        onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
      >
        Cancel
      </button>
      <button
        type="submit"
        className="flex-1 px-4 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 hover:opacity-90"
        style={{ background: 'var(--accent)', color: '#0c0b0a' }}
      >
        {submitLabel}
      </button>
    </div>
  )
}

// ─── Add Bookmark Modal ───────────────────────────────────────────────────────

interface AddBookmarkModalProps {
  folders: Folder[]
  tags: Tag[]
  onAdd: (data: { url: string; title: string; description: string; folder_id: string | null; tag_ids: string[]; feed_url: string | null }) => void
  onClose: () => void
}

function AddBookmarkModal({ folders, tags, onAdd, onClose }: AddBookmarkModalProps) {
  const [url, setUrl] = useState('')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [folderId, setFolderId] = useState('')
  const [selectedTags, setSelectedTags] = useState<Set<string>>(new Set())
  const urlRef = useRef<HTMLInputElement>(null)

  useEffect(() => { urlRef.current?.focus() }, [])

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
    onAdd({ url: url.trim(), title: title.trim(), description: description.trim(), folder_id: folderId || null, tag_ids: [...selectedTags], feed_url: null })
  }

  return (
    <ModalShell title="New Bookmark" onClose={onClose}>
      <form onSubmit={handleSubmit} className="p-6 flex flex-col gap-5">
        <div>
          <FieldLabel>URL *</FieldLabel>
          <input ref={urlRef} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com" className="ff" />
        </div>
        <div>
          <FieldLabel>Title *</FieldLabel>
          <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Page title" className="ff" />
        </div>
        <div>
          <FieldLabel>Note</FieldLabel>
          <textarea value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Optional note…" rows={2} className="ff" />
        </div>

        {folders.length > 0 && (
          <div>
            <FieldLabel>Folder</FieldLabel>
            <div className="relative">
              <select value={folderId} onChange={(e) => setFolderId(e.target.value)} className="ff pr-6">
                <option value="">No folder</option>
                {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
              </select>
              <span className="absolute right-0 top-1/2 -translate-y-1/2 pointer-events-none" style={{ color: 'var(--text-muted)' }}>
                <IconChevronDown size={13} />
              </span>
            </div>
          </div>
        )}

        {tags.length > 0 && (
          <div>
            <FieldLabel>Tags</FieldLabel>
            <div className="flex flex-wrap gap-2 pt-1">
              {tags.map((tag) => (
                <button
                  key={tag.id}
                  type="button"
                  onClick={() => toggleTag(tag.id)}
                  className="px-2.5 py-1 rounded text-xs font-medium transition-all duration-150"
                  style={
                    selectedTags.has(tag.id)
                      ? { background: tag.color + '28', color: tag.color, border: `1px solid ${tag.color}66` }
                      : { background: 'transparent', color: 'var(--text-muted)', border: '1px solid var(--border-mid)' }
                  }
                >
                  {tag.name}
                </button>
              ))}
            </div>
          </div>
        )}

        <ModalActions onClose={onClose} submitLabel="Save bookmark" />
      </form>
    </ModalShell>
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
    <ModalShell title="New Folder" onClose={onClose}>
      <form onSubmit={handleSubmit} className="p-6 flex flex-col gap-5">
        <div>
          <FieldLabel>Name</FieldLabel>
          <input ref={ref} value={name} onChange={(e) => setName(e.target.value)} placeholder="Folder name" className="ff" />
        </div>
        <ModalActions onClose={onClose} submitLabel="Create folder" />
      </form>
    </ModalShell>
  )
}

// ─── Add Tag Modal ────────────────────────────────────────────────────────────

const TAG_COLORS = ['#bf8b5e', '#e07a5f', '#f2cc8f', '#81b29a', '#6a9fb5', '#a78bca', '#e8a0b4', '#7fb5b5']

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
    <ModalShell title="New Tag" onClose={onClose}>
      <form onSubmit={handleSubmit} className="p-6 flex flex-col gap-5">
        <div>
          <FieldLabel>Name</FieldLabel>
          <input ref={ref} value={name} onChange={(e) => setName(e.target.value)} placeholder="Tag name" className="ff" />
        </div>
        <div>
          <FieldLabel>Color</FieldLabel>
          <div className="flex gap-2.5 pt-1">
            {TAG_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                onClick={() => setColor(c)}
                className="w-6 h-6 rounded-full transition-transform duration-100 relative"
                style={{ background: c, transform: color === c ? 'scale(1.2)' : 'scale(1)' }}
              >
                {color === c && (
                  <span className="absolute inset-0 flex items-center justify-center text-[#0c0b0a]">
                    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={3} strokeLinecap="round" strokeLinejoin="round">
                      <polyline points="20 6 9 17 4 12" />
                    </svg>
                  </span>
                )}
              </button>
            ))}
          </div>
        </div>
        <ModalActions onClose={onClose} submitLabel="Create tag" />
      </form>
    </ModalShell>
  )
}

// ─── Settings Modal ───────────────────────────────────────────────────────────

function SettingsModal({ onClose }: { onClose: () => void }) {
  const [token, setToken] = useState('')
  const [copied, setCopied] = useState(false)

  useEffect(() => { invoke<string>('get_api_token').then(setToken) }, [])

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
    <ModalShell title="Settings" onClose={onClose}>
      <div className="p-6 flex flex-col gap-6">
        {/* API Token */}
        <div>
          <FieldLabel>Browser Extension Token</FieldLabel>
          <p className="text-xs mb-3" style={{ color: 'var(--text-secondary)' }}>
            Paste into the Ferrico extension options page to connect it.
          </p>
          <div className="flex gap-2">
            <code
              className="flex-1 px-3 py-2 rounded-lg text-xs font-mono truncate"
              style={{ background: 'var(--bg-elevated)', border: '1px solid var(--border-dim)', color: 'var(--text-secondary)' }}
            >
              {token || '…'}
            </code>
            <button
              onClick={copy}
              className="px-3 py-2 rounded-lg text-xs font-medium transition-all duration-150"
              style={{
                background: copied ? 'var(--accent-dim)' : 'var(--bg-elevated)',
                border: '1px solid var(--border-mid)',
                color: copied ? 'var(--accent)' : 'var(--text-secondary)',
              }}
            >
              {copied ? 'Copied!' : 'Copy'}
            </button>
          </div>
        </div>

        {/* Export */}
        <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.5rem' }}>
          <FieldLabel>Export</FieldLabel>
          <p className="text-xs mb-3" style={{ color: 'var(--text-secondary)' }}>
            Download all bookmarks as an OPML file.
          </p>
          <button
            onClick={handleExport}
            className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all duration-150"
            style={{
              background: 'var(--bg-elevated)',
              border: '1px solid var(--border-mid)',
              color: 'var(--text-secondary)',
            }}
            onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
            onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
          >
            <IconExport size={14} />
            Export OPML
          </button>
        </div>
      </div>
    </ModalShell>
  )
}

// ─── Sidebar ──────────────────────────────────────────────────────────────────

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

function SidebarItem({ active, onClick, icon, label, count, onDelete }: {
  active: boolean
  onClick: () => void
  icon?: React.ReactNode
  label: string
  count?: number
  onDelete?: () => void
}) {
  const [hovered, setHovered] = useState(false)
  const [deleteHovered, setDeleteHovered] = useState(false)

  return (
    <button
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => { setHovered(false); setDeleteHovered(false) }}
      className="relative flex items-center gap-2 w-full px-3 py-1.5 rounded-lg text-sm transition-all duration-150 text-left group"
      style={{
        background: active ? 'var(--accent-dim)' : hovered ? 'rgba(255,255,255,0.035)' : 'transparent',
        color: active ? 'var(--accent)' : hovered ? 'var(--text-primary)' : 'var(--text-secondary)',
        borderLeft: active ? '2px solid var(--accent)' : '2px solid transparent',
        paddingLeft: active ? '10px' : '12px',
      }}
    >
      {icon && <span className="flex-none">{icon}</span>}
      <span className="flex-1 truncate font-medium">{label}</span>
      {count !== undefined && (
        <span className="text-xs flex-none" style={{ color: 'var(--text-muted)' }}>{count}</span>
      )}
      {onDelete && (hovered || deleteHovered) && (
        <span
          role="button"
          onClick={(e) => { e.stopPropagation(); onDelete() }}
          onMouseEnter={() => setDeleteHovered(true)}
          onMouseLeave={() => setDeleteHovered(false)}
          className="flex-none transition-colors duration-100"
          style={{ color: deleteHovered ? 'var(--red)' : 'var(--text-muted)' }}
        >
          <IconClose size={11} />
        </span>
      )}
    </button>
  )
}

function SidebarSection({ label, onAdd }: { label: string; onAdd: () => void }) {
  const [hovered, setHovered] = useState(false)
  return (
    <div className="flex items-center justify-between px-3 mb-1 mt-5">
      <span className="text-xs font-semibold uppercase tracking-widest" style={{ color: 'var(--text-muted)' }}>
        {label}
      </span>
      <button
        onClick={onAdd}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className="rounded p-0.5 transition-colors duration-150"
        style={{ color: hovered ? 'var(--text-primary)' : 'var(--text-muted)' }}
        title={`New ${label.toLowerCase()}`}
      >
        <IconPlus size={12} />
      </button>
    </div>
  )
}

function Sidebar({ folders, tags, selection, bookmarkCount, onSelect, onAddFolder, onDeleteFolder, onAddTag, onDeleteTag, onOpenSettings }: SidebarProps) {
  const isActive = (s: Selection): boolean => {
    if (s.type !== selection.type) return false
    if (s.type === 'all') return true
    if (s.type === 'folder' && selection.type === 'folder') return s.id === selection.id
    if (s.type === 'tag' && selection.type === 'tag') return s.id === selection.id
    return false
  }

  const [settingsHovered, setSettingsHovered] = useState(false)

  return (
    <aside
      className="w-52 flex-none flex flex-col h-full overflow-hidden"
      style={{ background: 'var(--bg-sidebar)', borderRight: '1px solid var(--border-dim)' }}
    >
      {/* Logo */}
      <div className="px-4 py-5 flex-none" style={{ borderBottom: '1px solid var(--border-dim)' }}>
        <div className="flex items-center gap-2.5">
          <span style={{ color: 'var(--accent)', lineHeight: 1 }}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor" stroke="none">
              <path d="M5 2a2 2 0 0 0-2 2v17.586a.5.5 0 0 0 .854.353L12 13.914l8.146 8.025A.5.5 0 0 0 21 21.586V4a2 2 0 0 0-2-2H5z"/>
            </svg>
          </span>
          <span
            className="text-base tracking-tight"
            style={{ fontFamily: "'Cormorant Garamond', serif", fontWeight: 500, color: 'var(--text-primary)', letterSpacing: '0.03em' }}
          >
            ferrico
          </span>
        </div>
      </div>

      {/* Nav */}
      <nav className="flex-1 overflow-y-auto px-2 py-3">
        <SidebarItem
          active={isActive({ type: 'all' })}
          onClick={() => onSelect({ type: 'all' })}
          icon={<IconAll />}
          label="All Bookmarks"
          count={bookmarkCount}
        />

        <SidebarSection label="Folders" onAdd={onAddFolder} />
        {folders.length === 0
          ? <p className="px-3 py-1 text-xs italic" style={{ color: 'var(--text-muted)' }}>No folders yet</p>
          : folders.map((folder) => (
            <SidebarItem
              key={folder.id}
              active={isActive({ type: 'folder', id: folder.id })}
              onClick={() => onSelect({ type: 'folder', id: folder.id })}
              icon={<IconFolder />}
              label={folder.name}
              onDelete={() => onDeleteFolder(folder.id)}
            />
          ))
        }

        <SidebarSection label="Tags" onAdd={onAddTag} />
        {tags.length === 0
          ? <p className="px-3 py-1 text-xs italic" style={{ color: 'var(--text-muted)' }}>No tags yet</p>
          : tags.map((tag) => (
            <SidebarItem
              key={tag.id}
              active={isActive({ type: 'tag', id: tag.id })}
              onClick={() => onSelect({ type: 'tag', id: tag.id })}
              icon={<span className="w-2 h-2 rounded-full flex-none block" style={{ background: tag.color }} />}
              label={tag.name}
              onDelete={() => onDeleteTag(tag.id)}
            />
          ))
        }
      </nav>

      {/* Settings */}
      <div className="flex-none px-2 py-3" style={{ borderTop: '1px solid var(--border-dim)' }}>
        <button
          onClick={onOpenSettings}
          onMouseEnter={() => setSettingsHovered(true)}
          onMouseLeave={() => setSettingsHovered(false)}
          className="flex items-center gap-2 w-full px-3 py-1.5 rounded-lg text-sm transition-colors duration-150"
          style={{
            color: settingsHovered ? 'var(--text-primary)' : 'var(--text-muted)',
            background: settingsHovered ? 'rgba(255,255,255,0.035)' : 'transparent',
          }}
        >
          <IconSettings />
          <span className="font-medium">Settings</span>
        </button>
      </div>
    </aside>
  )
}

// ─── Empty State ──────────────────────────────────────────────────────────────

function EmptyState({ onAdd }: { onAdd: () => void }) {
  const [btnHovered, setBtnHovered] = useState(false)

  return (
    <div className="anim-fade-in flex flex-col items-center justify-center h-full gap-5 text-center px-8">
      <div
        className="w-16 h-16 rounded-2xl flex items-center justify-center"
        style={{ background: 'var(--accent-dim)', border: '1px solid var(--border-mid)' }}
      >
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.25} strokeLinecap="round" strokeLinejoin="round" style={{ color: 'var(--accent)' }}>
          <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" />
        </svg>
      </div>
      <div>
        <p className="font-semibold text-base" style={{ color: 'var(--text-primary)' }}>Your library is empty</p>
        <p className="text-sm mt-1.5" style={{ color: 'var(--text-secondary)' }}>
          Add your first bookmark to start building your collection.
        </p>
      </div>
      <button
        onClick={onAdd}
        onMouseEnter={() => setBtnHovered(true)}
        onMouseLeave={() => setBtnHovered(false)}
        className="flex items-center gap-2 px-5 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150"
        style={{ background: 'var(--accent)', color: '#0c0b0a', opacity: btnHovered ? 0.88 : 1 }}
      >
        <IconPlus size={14} />
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
  const [addHovered, setAddHovered] = useState(false)
  const [searchFocused, setSearchFocused] = useState(false)

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
      />

      <div className="flex-1 flex flex-col min-w-0">
        {/* Error banner */}
        {error && (
          <div
            className="flex items-center justify-between px-6 py-2 gap-4 flex-none text-sm"
            style={{ background: 'rgba(224,82,82,0.1)', borderBottom: '1px solid rgba(224,82,82,0.2)', color: '#e07070' }}
          >
            <p className="truncate">{error}</p>
            <button onClick={() => setError(null)} className="text-xs flex-none hover:opacity-70 transition-opacity">Dismiss</button>
          </div>
        )}

        {/* Header */}
        <header
          className="flex items-center gap-4 px-6 py-3.5 flex-none"
          style={{ borderBottom: '1px solid var(--border-dim)', background: 'var(--bg-base)' }}
        >
          {/* Title */}
          <h1 className="font-semibold text-sm flex-none" style={{ color: 'var(--text-primary)' }}>
            {selectionTitle()}
          </h1>

          <div className="flex-1" />

          {/* Search */}
          <div
            className="flex items-center gap-2 px-3 py-2 rounded-lg transition-all duration-200 w-56"
            style={{
              background: 'var(--bg-elevated)',
              border: `1px solid ${searchFocused ? 'var(--border-bright)' : 'var(--border-dim)'}`,
              boxShadow: searchFocused ? '0 0 0 2px var(--accent-glow)' : 'none',
            }}
          >
            <span className="flex-none" style={{ color: searchFocused ? 'var(--accent)' : 'var(--text-muted)' }}>
              <IconSearch size={13} />
            </span>
            <input
              type="text"
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              onFocus={() => setSearchFocused(true)}
              onBlur={() => setSearchFocused(false)}
              placeholder="Search…"
              className="bg-transparent text-sm flex-1 min-w-0 outline-none"
              style={{ color: 'var(--text-primary)' }}
            />
            {searchInput && (
              <button
                onClick={() => setSearchInput('')}
                className="flex-none transition-colors duration-150"
                style={{ color: 'var(--text-muted)' }}
                onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-secondary)')}
                onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
              >
                <IconClose size={11} />
              </button>
            )}
          </div>

          {/* Add button */}
          <button
            onClick={() => setModal('add-bookmark')}
            onMouseEnter={() => setAddHovered(true)}
            onMouseLeave={() => setAddHovered(false)}
            className="flex items-center gap-1.5 px-3.5 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 flex-none"
            style={{ background: 'var(--accent)', color: '#0c0b0a', opacity: addHovered ? 0.88 : 1 }}
          >
            <IconPlus size={13} />
            Add
          </button>
        </header>

        {/* Column headers */}
        {bookmarks.length > 0 && (
          <div
            className="flex items-center gap-4 px-6 py-2 flex-none"
            style={{ borderBottom: '1px solid var(--border-dim)' }}
          >
            <div className="w-7 flex-none" />
            <span className="flex-1 text-xs uppercase tracking-widest font-medium" style={{ color: 'var(--text-muted)' }}>Title</span>
            <span className="hidden lg:block text-xs uppercase tracking-widest font-medium w-28 flex-none" style={{ color: 'var(--text-muted)' }}>Tags</span>
            <span className="hidden md:block text-xs uppercase tracking-widest font-medium w-20 text-right flex-none" style={{ color: 'var(--text-muted)' }}>Added</span>
            <div className="w-5 flex-none" />
          </div>
        )}

        {/* Content */}
        <main className="flex-1 overflow-y-auto">
          {bookmarks.length === 0 ? (
            <EmptyState onAdd={() => setModal('add-bookmark')} />
          ) : (
            <div>
              {bookmarks.map((b, i) => (
                <BookmarkRow key={b.id} bookmark={b} onDelete={handleDeleteBookmark} index={i} />
              ))}
            </div>
          )}
        </main>
      </div>

      {/* Modals */}
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
    </div>
  )
}
