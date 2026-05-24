import { useState } from 'react'
import type { Folder, Tag, Selection } from '../types'
import { IconClose, IconPlus, IconFolder, IconAll, IconInbox, IconSettings, IconTrash } from './icons'

export interface SidebarProps {
  folders: Folder[]
  tags: Tag[]
  selection: Selection
  bookmarkCount: number
  inboxCount?: number
  binCount: number
  onSelect: (s: Selection) => void
  onAddFolder: () => void
  onDeleteFolder: (id: string) => void
  onAddTag: () => void
  onDeleteTag: (id: string) => void
  onOpenSettings: () => void
  onFolderContext: (e: React.MouseEvent, folder: Folder) => void
  onTagContext: (e: React.MouseEvent, tag: Tag) => void
  onDropBookmark?: (bookmarkId: string, folderId: string | null) => void
}

export function SidebarItem({ active, onClick, onContext, icon, label, count, onDelete, ariaLabel, folderId, onDropBookmark }: {
  active: boolean
  onClick: () => void
  onContext?: (e: React.MouseEvent) => void
  icon?: React.ReactNode
  label: string
  count?: number
  onDelete?: () => void
  ariaLabel?: string
  folderId?: string | null
  onDropBookmark?: (bookmarkId: string, folderId: string | null) => void
}) {
  const [hovered, setHovered] = useState(false)
  const [deleteHovered, setDeleteHovered] = useState(false)
  const [dragOver, setDragOver] = useState(false)

  const handleDragOver = (e: React.DragEvent) => {
    if (folderId === undefined) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    setDragOver(true)
  }

  const handleDragLeave = (e: React.DragEvent) => {
    // Only clear when the cursor actually leaves the button, not when entering a child
    if (!e.currentTarget.contains(e.relatedTarget as Node)) {
      setDragOver(false)
    }
  }

  const handleDrop = (e: React.DragEvent) => {
    if (folderId === undefined) return
    e.preventDefault()
    setDragOver(false)
    const bookmarkId = e.dataTransfer.getData('text/plain')
    // Pass id (may be empty string); App's handleDropBookmark falls back to ref
    onDropBookmark?.(bookmarkId, folderId)
  }

  return (
    <button
      onClick={onClick}
      onContextMenu={onContext}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => { setHovered(false); setDeleteHovered(false) }}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      className="relative flex items-center gap-2 w-full px-3 py-1.5 rounded-lg text-sm transition-all duration-75 text-left group cursor-pointer"
      style={{
        background: dragOver ? 'var(--accent)' : active ? 'var(--accent-dim)' : hovered ? 'rgba(255,255,255,0.035)' : 'transparent',
        color: dragOver ? '#0c0b0a' : active ? 'var(--accent)' : hovered ? 'var(--text-primary)' : 'var(--text-secondary)',
        outline: dragOver ? '2px solid var(--accent-bright)' : 'none',
        outlineOffset: '1px',
        borderLeft: active && !dragOver ? '2px solid var(--accent)' : '2px solid transparent',
        paddingLeft: active && !dragOver ? '10px' : '12px',
      }}
      aria-current={active ? 'page' : undefined}
      aria-label={ariaLabel}
    >
      {icon && <span className="flex-none" aria-hidden="true">{icon}</span>}
      <span className="flex-1 truncate font-medium">{label}</span>
      {count !== undefined && (
        <span className="text-xs flex-none" style={{ color: 'var(--text-muted)' }} aria-label={`${count} bookmarks`}>{count}</span>
      )}
      {onDelete && (hovered || deleteHovered) && (
        <button
          type="button"
          onClick={(e) => { e.stopPropagation(); onDelete() }}
          onMouseEnter={() => setDeleteHovered(true)}
          onMouseLeave={() => setDeleteHovered(false)}
          className="flex-none transition-colors duration-100 p-0.5 rounded cursor-pointer"
          style={{ color: deleteHovered ? 'var(--red)' : 'var(--text-muted)' }}
          aria-label={`Delete ${label}`}
        >
          <IconClose size={11} />
        </button>
      )}
    </button>
  )
}

export function SidebarSection({ label, onAdd }: { label: string; onAdd: () => void }) {
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
        className="rounded p-0.5 transition-colors duration-150 cursor-pointer"
        style={{ color: hovered ? 'var(--text-primary)' : 'var(--text-muted)' }}
        aria-label={`New ${label.toLowerCase()}`}
      >
        <IconPlus size={12} />
      </button>
    </div>
  )
}

export function Sidebar({ folders, tags, selection, bookmarkCount, inboxCount = 0, binCount, onSelect, onAddFolder, onDeleteFolder, onAddTag, onDeleteTag, onOpenSettings, onFolderContext, onTagContext, onDropBookmark }: SidebarProps) {
  const isActive = (s: Selection): boolean => {
    if (s.type !== selection.type) return false
    if (s.type === 'all') return true
    if (s.type === 'inbox') return true
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
      <div className="px-4 py-5 flex-none" style={{ borderBottom: '1px solid var(--border-dim)' }}>
        <div className="flex items-center gap-2.5">
          <span style={{ color: 'var(--accent)', lineHeight: 1 }} aria-hidden="true">
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

      <nav className="flex-1 overflow-y-auto px-2 py-3" aria-label="Library navigation">
        <SidebarItem
          active={isActive({ type: 'inbox' })}
          onClick={() => onSelect({ type: 'inbox' })}
          icon={<IconInbox />}
          label="Inbox"
          count={inboxCount}
          ariaLabel={`Inbox, ${inboxCount} unsorted`}
          folderId={null}
          onDropBookmark={onDropBookmark}
        />
        <SidebarItem
          active={isActive({ type: 'all' })}
          onClick={() => onSelect({ type: 'all' })}
          icon={<IconAll />}
          label="All Bookmarks"
          count={bookmarkCount}
          ariaLabel={`All Bookmarks, ${bookmarkCount} total`}
        />

        <SidebarItem
          active={isActive({ type: 'bin' })}
          onClick={() => onSelect({ type: 'bin' })}
          icon={<IconTrash />}
          label="Bin"
          count={binCount > 0 ? binCount : undefined}
          ariaLabel={binCount > 0 ? `Bin, ${binCount} items` : 'Bin, empty'}
        />

        <SidebarSection label="Folders" onAdd={onAddFolder} />
        {folders.length === 0
          ? <p className="px-3 py-1 text-xs italic" style={{ color: 'var(--text-muted)' }}>No folders yet</p>
          : folders.map((folder) => (
            <SidebarItem
              key={folder.id}
              active={isActive({ type: 'folder', id: folder.id })}
              onClick={() => onSelect({ type: 'folder', id: folder.id })}
              onContext={(e) => onFolderContext(e, folder)}
              icon={<IconFolder />}
              label={folder.name}
              onDelete={() => onDeleteFolder(folder.id)}
              folderId={folder.id}
              onDropBookmark={onDropBookmark}
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
              onContext={(e) => onTagContext(e, tag)}
              icon={<span className="w-2 h-2 rounded-full flex-none block" style={{ background: tag.color }} aria-hidden="true" />}
              label={tag.name}
              onDelete={() => onDeleteTag(tag.id)}
            />
          ))
        }
      </nav>

      <div className="flex-none px-2 py-3" style={{ borderTop: '1px solid var(--border-dim)' }}>
        <button
          onClick={onOpenSettings}
          onMouseEnter={() => setSettingsHovered(true)}
          onMouseLeave={() => setSettingsHovered(false)}
          className="flex items-center gap-2 w-full px-3 py-1.5 rounded-lg text-sm transition-colors duration-150 cursor-pointer"
          style={{
            color: settingsHovered ? 'var(--text-primary)' : 'var(--text-muted)',
            background: settingsHovered ? 'rgba(255,255,255,0.035)' : 'transparent',
          }}
          aria-label="Open settings"
        >
          <IconSettings />
          <span className="font-medium">Settings</span>
        </button>
      </div>
    </aside>
  )
}
