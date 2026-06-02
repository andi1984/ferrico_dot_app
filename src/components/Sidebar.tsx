import { Fragment, useMemo, useState } from 'react'
import type { Folder, Tag, Selection } from '../types'
import { IconClose, IconPlus, IconFolder, IconAll, IconInbox, IconSettings, IconTrash, IconBrokenLink, IconChevronDown } from './icons'

// Sentinel used in [data-drop-target-id] for the Inbox row, which corresponds
// to "unsorted" (folderId === null). The App layer maps it back to null when
// invoking the Tauri command.
export const INBOX_DROP_TARGET = '__inbox__'

// Sentinel drop target on the "Folders" section header. Dropping a dragged
// folder here re-parents it to the top level (parent_id = null).
export const FOLDER_ROOT_DROP_TARGET = '__folder_root__'

// What kind of item is currently being dragged, so the sidebar can highlight
// only the drop targets that actually accept it (folders accept both; Inbox
// accepts bookmarks; the Folders header accepts folders).
export type DragKind = 'bookmark' | 'folder' | null

// Horizontal indent applied per nesting level for subfolders.
const FOLDER_INDENT_STEP = 14

export interface SidebarProps {
  folders: Folder[]
  tags: Tag[]
  selection: Selection
  bookmarkCount: number
  inboxCount?: number
  binCount: number
  brokenCount?: number
  onSelect: (s: Selection) => void
  onAddFolder: () => void
  onDeleteFolder: (id: string) => void
  onAddTag: () => void
  onDeleteTag: (id: string) => void
  onOpenSettings: () => void
  onFolderContext: (e: React.MouseEvent, folder: Folder) => void
  onTagContext: (e: React.MouseEvent, tag: Tag) => void
  // Begins a folder drag (pointer-based) so folders can be re-parented.
  onFolderPointerDown?: (e: React.PointerEvent, folder: Folder) => void
  // ID of the drop target currently hovered during a drag (or null). Used to
  // paint a highlight on the matching SidebarItem.
  dragHoverTargetId?: string | null
  // What is being dragged, so only valid targets light up. See DragKind.
  dragKind?: DragKind
}

export function SidebarItem({ active, onClick, onContext, onPointerDown, icon, label, count, onDelete, ariaLabel, dropTargetId, isDragTarget, indentLevel = 0, chevron }: {
  active: boolean
  onClick: () => void
  onContext?: (e: React.MouseEvent) => void
  onPointerDown?: (e: React.PointerEvent) => void
  icon?: React.ReactNode
  label: string
  count?: number
  onDelete?: () => void
  ariaLabel?: string
  dropTargetId?: string
  isDragTarget?: boolean
  // Nesting depth (0 = top level). Adds left padding so subfolders read as a tree.
  indentLevel?: number
  // Optional expand/collapse control rendered at the start of the row. When the
  // row sits in a tree but has no children, pass a spacer to keep icons aligned.
  chevron?: React.ReactNode
}) {
  const [hovered, setHovered] = useState(false)
  const [deleteHovered, setDeleteHovered] = useState(false)

  return (
    <button
      onClick={onClick}
      onContextMenu={onContext}
      onPointerDown={onPointerDown}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => { setHovered(false); setDeleteHovered(false) }}
      data-drop-target-id={dropTargetId}
      className={`relative flex items-center gap-2.5 w-full rounded-md text-left transition-colors duration-150 group ${onPointerDown ? 'cursor-grab select-none' : 'cursor-pointer'}`}
      style={{
        padding: '6px 10px',
        paddingLeft: 10 + indentLevel * FOLDER_INDENT_STEP,
        // Suppress native text-selection/drag so our pointer-drag wins in the
        // webview (WKWebView on macOS), matching the bookmark rows.
        touchAction: onPointerDown ? 'none' : undefined,
        background: isDragTarget
          ? 'var(--accent)'
          : active
            ? 'var(--row-sel-bg)'
            : hovered
              ? 'var(--row-hover-bg)'
              : 'transparent',
        color: isDragTarget ? '#1a1410' : 'var(--text-1)',
        outline: isDragTarget ? '2px solid var(--accent-bright)' : 'none',
        outlineOffset: '1px',
      }}
      aria-current={active ? 'page' : undefined}
      aria-label={ariaLabel}
    >
      {active && !isDragTarget && (
        <span
          aria-hidden="true"
          className="absolute left-0 top-1/2 -translate-y-1/2 rounded-r-full"
          style={{ width: 2.5, height: 14, background: 'var(--accent)' }}
        />
      )}
      {chevron}
      {icon && (
        <span
          className="flex-none flex items-center justify-center"
          aria-hidden="true"
          style={{
            color: isDragTarget
              ? '#1a1410'
              : active
                ? 'var(--accent)'
                : 'var(--text-2)',
            opacity: active ? 1 : 0.85,
          }}
        >
          {icon}
        </span>
      )}
      <span
        className="flex-1 truncate"
        style={{
          fontSize: 13,
          fontWeight: active ? 600 : 500,
          letterSpacing: '-0.005em',
        }}
      >{label}</span>
      {count !== undefined && (
        <span
          className="flex-none tabnum mono"
          style={{
            fontSize: 11,
            color: isDragTarget ? '#1a1410' : active ? 'var(--accent)' : 'var(--text-3)',
            fontWeight: 500,
          }}
          aria-label={`${count} bookmarks`}
        >{count.toLocaleString()}</span>
      )}
      {onDelete && !isDragTarget && (hovered || deleteHovered) && (
        <button
          type="button"
          onClick={(e) => { e.stopPropagation(); onDelete() }}
          onMouseEnter={() => setDeleteHovered(true)}
          onMouseLeave={() => setDeleteHovered(false)}
          className="flex-none transition-colors duration-100 p-0.5 rounded cursor-pointer"
          style={{ color: deleteHovered ? 'var(--red)' : 'var(--text-3)' }}
          aria-label={`Delete ${label}`}
        >
          <IconClose size={11} />
        </button>
      )}
    </button>
  )
}

export function SidebarSection({ label, onAdd, dropTargetId, isDragTarget }: {
  label: string
  onAdd: () => void
  dropTargetId?: string
  isDragTarget?: boolean
}) {
  const [hovered, setHovered] = useState(false)
  return (
    <div
      data-drop-target-id={dropTargetId}
      className="flex items-center justify-between px-4 mt-5 mb-1 rounded-md"
      style={{
        background: isDragTarget ? 'var(--accent)' : 'transparent',
        outline: isDragTarget ? '2px solid var(--accent-bright)' : 'none',
        outlineOffset: '1px',
      }}
    >
      <span className="section-label" style={isDragTarget ? { color: '#1a1410' } : undefined}>{label}</span>
      <button
        onClick={onAdd}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className="rounded transition-colors duration-150 cursor-pointer flex items-center justify-center"
        style={{
          width: 18,
          height: 18,
          color: hovered ? 'var(--text-1)' : 'var(--text-3)',
          background: hovered ? 'var(--row-hover-bg)' : 'transparent',
        }}
        aria-label={`New ${label.toLowerCase()}`}
      >
        <IconPlus size={12} />
      </button>
    </div>
  )
}

export function Sidebar({ folders, tags, selection, bookmarkCount, inboxCount = 0, binCount, brokenCount = 0, onSelect, onAddFolder, onDeleteFolder, onAddTag, onDeleteTag, onOpenSettings, onFolderContext, onTagContext, onFolderPointerDown, dragHoverTargetId, dragKind = null }: SidebarProps) {
  // Group folders by parent so we can render the tree. `folders` arrives sorted
  // by name from the DB, so siblings keep that order. A folder is a root when
  // its parent_id is null (or points at a folder that no longer exists).
  const childrenByParent = useMemo(() => {
    const ids = new Set(folders.map((f) => f.id))
    const map = new Map<string | null, Folder[]>()
    for (const f of folders) {
      const key = f.parent_id && ids.has(f.parent_id) ? f.parent_id : null
      const arr = map.get(key)
      if (arr) arr.push(f)
      else map.set(key, [f])
    }
    return map
  }, [folders])

  // Collapsed folder ids. Folders are expanded by default; toggling adds/removes.
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set())
  const toggleCollapsed = (id: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const isActive = (s: Selection): boolean => {
    if (s.type !== selection.type) return false
    if (s.type === 'folder' && selection.type === 'folder') return s.id === selection.id
    if (s.type === 'tag' && selection.type === 'tag') return s.id === selection.id
    return true
  }

  const [settingsHovered, setSettingsHovered] = useState(false)

  // Render a folder and, when expanded, its subfolders — depth-first.
  const renderFolder = (folder: Folder, level: number): React.ReactNode => {
    const children = childrenByParent.get(folder.id) ?? []
    const hasChildren = children.length > 0
    const isCollapsed = collapsed.has(folder.id)
    const chevron = (
      <span className="flex-none flex items-center justify-center" style={{ width: 14 }}>
        {hasChildren && (
          <button
            type="button"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => { e.stopPropagation(); toggleCollapsed(folder.id) }}
            className="flex items-center justify-center rounded cursor-pointer"
            style={{ width: 14, height: 14, color: 'var(--text-3)' }}
            aria-label={isCollapsed ? `Expand ${folder.name}` : `Collapse ${folder.name}`}
            aria-expanded={!isCollapsed}
          >
            <span style={{ display: 'inline-flex', transition: 'transform 150ms', transform: isCollapsed ? 'rotate(-90deg)' : 'none' }}>
              <IconChevronDown size={12} />
            </span>
          </button>
        )}
      </span>
    )
    return (
      <Fragment key={folder.id}>
        <SidebarItem
          active={isActive({ type: 'folder', id: folder.id })}
          onClick={() => onSelect({ type: 'folder', id: folder.id })}
          onContext={(e) => onFolderContext(e, folder)}
          onPointerDown={onFolderPointerDown ? (e) => onFolderPointerDown(e, folder) : undefined}
          icon={<IconFolder />}
          label={folder.name}
          onDelete={() => onDeleteFolder(folder.id)}
          dropTargetId={folder.id}
          isDragTarget={dragHoverTargetId === folder.id}
          indentLevel={level}
          chevron={chevron}
        />
        {hasChildren && !isCollapsed && children.map((child) => renderFolder(child, level + 1))}
      </Fragment>
    )
  }

  const rootFolders = childrenByParent.get(null) ?? []

  return (
    <aside
      className="flex flex-col shrink-0 h-full overflow-hidden"
      style={{
        width: 224,
        background: 'var(--sidebar-bg)',
        borderRight: '1px solid var(--border-soft)',
      }}
    >
      {/* Brand block */}
      <div className="flex items-center gap-2.5 px-4 pt-4 pb-5 shrink-0">
        <div
          className="flex items-center justify-center shrink-0"
          style={{
            width: 28,
            height: 28,
            borderRadius: 8,
            background: 'linear-gradient(135deg, var(--accent) 0%, var(--accent-deep) 100%)',
            boxShadow:
              'inset 0 1px 0 rgba(255,255,255,0.08), 0 4px 12px -2px rgba(0,0,0,0.4)',
          }}
          aria-hidden="true"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="#1a1410" stroke="none">
            <path d="M6 3h12a1 1 0 0 1 1 1v17l-7-4-7 4V4a1 1 0 0 1 1-1z" />
          </svg>
        </div>
        <div className="flex flex-col leading-none min-w-0">
          <span
            style={{
              fontFamily: 'var(--font-display)',
              fontSize: 17,
              fontWeight: 600,
              color: 'var(--text-1)',
              letterSpacing: '-0.01em',
            }}
          >ferrico</span>
          <span
            className="mono truncate"
            style={{
              fontSize: 10,
              color: 'var(--text-3)',
              marginTop: 3,
              letterSpacing: '0.02em',
            }}
          >v0.1 · {bookmarkCount.toLocaleString()} marks</span>
        </div>
      </div>

      <nav className="flex-1 overflow-y-auto sb-scroll px-2 pb-3 flex flex-col gap-0.5" aria-label="Library navigation">
        <SidebarItem
          active={isActive({ type: 'inbox' })}
          onClick={() => onSelect({ type: 'inbox' })}
          icon={<IconInbox />}
          label="Inbox"
          count={inboxCount}
          ariaLabel={`Inbox, ${inboxCount} unsorted`}
          dropTargetId={INBOX_DROP_TARGET}
          isDragTarget={dragKind === 'bookmark' && dragHoverTargetId === INBOX_DROP_TARGET}
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

        <SidebarItem
          active={isActive({ type: 'broken' })}
          onClick={() => onSelect({ type: 'broken' })}
          icon={<IconBrokenLink />}
          label="Broken Links"
          count={brokenCount > 0 ? brokenCount : undefined}
          ariaLabel={brokenCount > 0 ? `Broken Links, ${brokenCount} items` : 'Broken Links'}
        />

        <SidebarSection
          label="Folders"
          onAdd={onAddFolder}
          dropTargetId={FOLDER_ROOT_DROP_TARGET}
          isDragTarget={dragKind === 'folder' && dragHoverTargetId === FOLDER_ROOT_DROP_TARGET}
        />
        {rootFolders.length === 0
          ? <p className="px-3 py-1 italic" style={{ color: 'var(--text-3)', fontSize: 11.5 }}>No folders yet</p>
          : rootFolders.map((folder) => renderFolder(folder, 0))
        }

        <SidebarSection label="Tags" onAdd={onAddTag} />
        {tags.length === 0
          ? <p className="px-3 py-1 italic" style={{ color: 'var(--text-3)', fontSize: 11.5 }}>No tags yet</p>
          : tags.map((tag) => (
            <SidebarItem
              key={tag.id}
              active={isActive({ type: 'tag', id: tag.id })}
              onClick={() => onSelect({ type: 'tag', id: tag.id })}
              onContext={(e) => onTagContext(e, tag)}
              icon={
                <span
                  className="block rounded-full"
                  style={{ width: 6, height: 6, background: tag.color }}
                  aria-hidden="true"
                />
              }
              label={tag.name}
              onDelete={() => onDeleteTag(tag.id)}
            />
          ))
        }
      </nav>

      <div className="shrink-0" style={{ borderTop: '1px solid var(--border-soft)' }}>
        <button
          onClick={onOpenSettings}
          onMouseEnter={() => setSettingsHovered(true)}
          onMouseLeave={() => setSettingsHovered(false)}
          className="w-full flex items-center gap-2.5 px-4 py-3 transition-colors duration-150 cursor-pointer"
          style={{
            color: settingsHovered ? 'var(--text-1)' : 'var(--text-2)',
            background: settingsHovered ? 'var(--row-hover-bg)' : 'transparent',
          }}
          aria-label="Open settings"
        >
          <IconSettings size={15} />
          <span style={{ fontSize: 13, fontWeight: 500 }}>Settings</span>
          <span
            className="ml-auto mono"
            style={{
              fontSize: 10,
              color: 'var(--text-3)',
              padding: '2px 5px',
              border: '1px solid var(--border)',
              borderRadius: 4,
            }}
          >⌘,</span>
        </button>
      </div>
    </aside>
  )
}
