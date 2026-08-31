import { useEffect } from 'react'
import type { Counts, Folder, Tag } from '../types'
import type { MobileSelection } from './MobileApp'
import { IconAll, IconClose, IconFolder, IconInbox, IconMore, IconPlus, IconTrash } from '../components/icons'

interface FilterDrawerProps {
  open: boolean
  onClose: () => void
  folders: Folder[]
  tags: Tag[]
  counts: Counts
  selection: MobileSelection
  onSelect: (selection: MobileSelection) => void
  /** Opens the New Folder modal (top-level). */
  onAddFolder: () => void
  /** Opens the New Tag modal. */
  onAddTag: () => void
  /** Opens the folder action sheet (new subfolder / delete). `level` is the
      0-based nesting depth, used for the subfolder depth cap. */
  onFolderMenu: (folder: Folder, level: number) => void
  /** Opens the tag action sheet (delete). */
  onTagMenu: (tag: Tag) => void
}

// Horizontal indent per nesting level, mirroring the desktop Sidebar's tree.
const INDENT_STEP = 16

function selectionKey(sel: MobileSelection): string {
  return sel.type === 'folder' || sel.type === 'tag' ? `${sel.type}:${sel.id}` : sel.type
}

// Bottom-sheet drawer for navigation and folder/tag management. Deliberately
// not a reuse of the desktop Sidebar (drag-drop, context menus, collapse
// state) — tap-to-select-and-close plus "+" / "⋮" management buttons is all
// mobile needs.
export function FilterDrawer({
  open,
  onClose,
  folders,
  tags,
  counts,
  selection,
  onSelect,
  onAddFolder,
  onAddTag,
  onFolderMenu,
  onTagMenu,
}: FilterDrawerProps) {
  useEffect(() => {
    if (!open) return
    const prevOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => { document.body.style.overflow = prevOverflow }
  }, [open])

  if (!open) return null

  const activeKey = selectionKey(selection)
  const pick = (sel: MobileSelection) => {
    onSelect(sel)
    onClose()
  }

  const childrenByParent = new Map<string | null, Folder[]>()
  const ids = new Set(folders.map((f) => f.id))
  for (const f of folders) {
    const key = f.parent_id && ids.has(f.parent_id) ? f.parent_id : null
    const arr = childrenByParent.get(key)
    if (arr) arr.push(f)
    else childrenByParent.set(key, [f])
  }
  const rootFolders = childrenByParent.get(null) ?? []

  const plainRow = (sel: MobileSelection, icon: React.ReactNode, label: string, count: number) => {
    const key = selectionKey(sel)
    const isActive = activeKey === key
    return (
      <button
        className={`filter-row${isActive ? ' is-active' : ''}`}
        onClick={() => pick(sel)}
        aria-current={isActive ? 'page' : undefined}
      >
        {icon}
        <span className="filter-row-label">{label}</span>
        <span className="filter-row-count" aria-label={`${count} bookmarks`}>{count}</span>
      </button>
    )
  }

  const renderFolder = (folder: Folder, level: number): React.ReactNode => {
    const key = selectionKey({ type: 'folder', id: folder.id })
    const isActive = activeKey === key
    const children = childrenByParent.get(folder.id) ?? []
    return (
      <div key={folder.id}>
        <div className="filter-row-wrap">
          <button
            className={`filter-row${isActive ? ' is-active' : ''}`}
            style={{ paddingLeft: 16 + level * INDENT_STEP }}
            onClick={() => pick({ type: 'folder', id: folder.id })}
            aria-current={isActive ? 'page' : undefined}
          >
            <IconFolder size={15} />
            <span className="filter-row-label">{folder.name}</span>
            <span className="filter-row-count" aria-label={`${folder.bookmark_count ?? 0} bookmarks`}>
              {folder.bookmark_count ?? 0}
            </span>
          </button>
          <button
            className="filter-row-menu"
            onClick={() => onFolderMenu(folder, level)}
            aria-label={`Actions for folder ${folder.name}`}
          >
            <IconMore size={16} />
          </button>
        </div>
        {children.map((child) => renderFolder(child, level + 1))}
      </div>
    )
  }

  return (
    <>
      <div className="filter-drawer-backdrop" onClick={onClose} aria-hidden="true" />
      <div className="filter-drawer" role="dialog" aria-modal="true" aria-label="Filter by folder or tag">
        <div className="filter-drawer-header">
          <h2 className="filter-drawer-title">Filter</h2>
          <button className="mobile-icon-btn" onClick={onClose} aria-label="Close filter">
            <IconClose size={16} />
          </button>
        </div>

        <div className="filter-drawer-body">
          {plainRow({ type: 'all' }, <IconAll size={15} />, 'All bookmarks', counts.total)}
          {plainRow({ type: 'inbox' }, <IconInbox size={15} />, 'Inbox', counts.inbox)}
          {plainRow({ type: 'bin' }, <IconTrash size={15} />, 'Bin', counts.bin)}

          <div className="filter-section-head">
            <p className="filter-section-label">Folders</p>
            <button className="filter-section-add" onClick={onAddFolder} aria-label="New folder">
              <IconPlus size={14} />
            </button>
          </div>
          {rootFolders.length === 0 ? (
            <p className="filter-empty">No folders yet</p>
          ) : (
            rootFolders.map((folder) => renderFolder(folder, 0))
          )}

          <div className="filter-section-head">
            <p className="filter-section-label">Tags</p>
            <button className="filter-section-add" onClick={onAddTag} aria-label="New tag">
              <IconPlus size={14} />
            </button>
          </div>
          {tags.length === 0 ? (
            <p className="filter-empty">No tags yet</p>
          ) : (
            tags.map((tag) => {
              const key = selectionKey({ type: 'tag', id: tag.id })
              const isActive = activeKey === key
              return (
                <div className="filter-row-wrap" key={tag.id}>
                  <button
                    className={`filter-row${isActive ? ' is-active' : ''}`}
                    onClick={() => pick({ type: 'tag', id: tag.id })}
                    aria-current={isActive ? 'page' : undefined}
                  >
                    <span className="filter-row-dot" style={{ background: tag.color }} aria-hidden="true" />
                    <span className="filter-row-label">{tag.name}</span>
                    <span className="filter-row-count" aria-label={`${tag.bookmark_count ?? 0} bookmarks`}>
                      {tag.bookmark_count ?? 0}
                    </span>
                  </button>
                  <button
                    className="filter-row-menu"
                    onClick={() => onTagMenu(tag)}
                    aria-label={`Actions for tag ${tag.name}`}
                  >
                    <IconMore size={16} />
                  </button>
                </div>
              )
            })
          )}
        </div>
      </div>
    </>
  )
}
