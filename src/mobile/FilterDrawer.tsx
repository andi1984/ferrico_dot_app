import { useEffect } from 'react'
import type { Counts, Folder, Tag } from '../types'
import type { MobileSelection } from './MobileApp'
import { IconAll, IconClose, IconFolder } from '../components/icons'

interface FilterDrawerProps {
  open: boolean
  onClose: () => void
  folders: Folder[]
  tags: Tag[]
  counts: Counts
  selection: MobileSelection
  onSelect: (selection: MobileSelection) => void
}

// Horizontal indent per nesting level, mirroring the desktop Sidebar's tree.
const INDENT_STEP = 16

function selectionKey(sel: MobileSelection): string {
  return sel.type === 'all' ? 'all' : `${sel.type}:${sel.id}`
}

// Simple bottom-sheet filter drawer for folder/tag navigation. Deliberately not
// a reuse of the desktop Sidebar (drag-drop, context menus, collapse state) —
// read-only, tap-to-select-and-close is all mobile needs.
export function FilterDrawer({ open, onClose, folders, tags, counts, selection, onSelect }: FilterDrawerProps) {
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

  const renderFolder = (folder: Folder, level: number): React.ReactNode => {
    const key = selectionKey({ type: 'folder', id: folder.id })
    const isActive = activeKey === key
    const children = childrenByParent.get(folder.id) ?? []
    return (
      <div key={folder.id}>
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
          <button
            className={`filter-row${activeKey === 'all' ? ' is-active' : ''}`}
            onClick={() => pick({ type: 'all' })}
            aria-current={activeKey === 'all' ? 'page' : undefined}
          >
            <IconAll size={15} />
            <span className="filter-row-label">All bookmarks</span>
            <span className="filter-row-count" aria-label={`${counts.total} bookmarks`}>{counts.total}</span>
          </button>

          <p className="filter-section-label">Folders</p>
          {rootFolders.length === 0 ? (
            <p className="filter-empty">No folders yet</p>
          ) : (
            rootFolders.map((folder) => renderFolder(folder, 0))
          )}

          <p className="filter-section-label">Tags</p>
          {tags.length === 0 ? (
            <p className="filter-empty">No tags yet</p>
          ) : (
            tags.map((tag) => {
              const key = selectionKey({ type: 'tag', id: tag.id })
              const isActive = activeKey === key
              return (
                <button
                  key={tag.id}
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
              )
            })
          )}
        </div>
      </div>
    </>
  )
}
