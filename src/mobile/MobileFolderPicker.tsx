import { useEffect } from 'react'
import type { Folder } from '../types'
import { IconClose, IconFolder, IconInbox } from '../components/icons'

interface MobileFolderPickerProps {
  open: boolean
  folders: Folder[]
  /** null = Inbox (no folder). */
  onPick: (folderId: string | null) => void
  onClose: () => void
}

const INDENT_STEP = 16

// Bottom-sheet destination picker for "Move to folder…" — the touch
// counterpart of the desktop's drag-to-sidebar move.
export function MobileFolderPicker({ open, folders, onPick, onClose }: MobileFolderPickerProps) {
  useEffect(() => {
    if (!open) return
    const prevOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => { document.body.style.overflow = prevOverflow }
  }, [open])

  if (!open) return null

  const childrenByParent = new Map<string | null, Folder[]>()
  const ids = new Set(folders.map((f) => f.id))
  for (const f of folders) {
    const key = f.parent_id && ids.has(f.parent_id) ? f.parent_id : null
    const arr = childrenByParent.get(key)
    if (arr) arr.push(f)
    else childrenByParent.set(key, [f])
  }

  const pick = (folderId: string | null) => {
    onPick(folderId)
    onClose()
  }

  const renderFolder = (folder: Folder, level: number): React.ReactNode => {
    const children = childrenByParent.get(folder.id) ?? []
    return (
      <div key={folder.id}>
        <button
          type="button"
          className="filter-row"
          style={{ paddingLeft: 16 + level * INDENT_STEP }}
          onClick={() => pick(folder.id)}
        >
          <IconFolder size={15} />
          <span className="filter-row-label">{folder.name}</span>
        </button>
        {children.map((child) => renderFolder(child, level + 1))}
      </div>
    )
  }

  return (
    <>
      <div className="filter-drawer-backdrop" onClick={onClose} aria-hidden="true" />
      <div className="filter-drawer" role="dialog" aria-modal="true" aria-label="Move to folder">
        <div className="filter-drawer-header">
          <h2 className="filter-drawer-title">Move to</h2>
          <button className="mobile-icon-btn" onClick={onClose} aria-label="Close folder picker">
            <IconClose size={16} />
          </button>
        </div>
        <div className="filter-drawer-body">
          <button type="button" className="filter-row" onClick={() => pick(null)}>
            <IconInbox size={15} />
            <span className="filter-row-label">Inbox (no folder)</span>
          </button>
          {(childrenByParent.get(null) ?? []).map((folder) => renderFolder(folder, 0))}
        </div>
      </div>
    </>
  )
}
