import { useEffect } from 'react'
import { IconClose } from '../components/icons'

export interface SheetAction {
  label: string
  icon?: React.ReactNode
  /** Renders the row in the destructive color. */
  danger?: boolean
  onPress: () => void
}

interface MobileActionSheetProps {
  open: boolean
  title?: string
  actions: SheetAction[]
  onClose: () => void
}

// Bottom action sheet — the mobile stand-in for the desktop ContextMenu.
// Opened from a row/card's "⋮" button; each action closes the sheet before
// running so a follow-up sheet (e.g. the folder picker) can take its place.
export function MobileActionSheet({ open, title, actions, onClose }: MobileActionSheetProps) {
  useEffect(() => {
    if (!open) return
    const prevOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => { document.body.style.overflow = prevOverflow }
  }, [open])

  if (!open) return null

  return (
    <>
      <div className="filter-drawer-backdrop" onClick={onClose} aria-hidden="true" />
      <div className="filter-drawer" role="dialog" aria-modal="true" aria-label={title ?? 'Actions'}>
        <div className="filter-drawer-header">
          <h2 className="filter-drawer-title truncate">{title ?? 'Actions'}</h2>
          <button className="mobile-icon-btn" onClick={onClose} aria-label="Close actions">
            <IconClose size={16} />
          </button>
        </div>
        <div className="filter-drawer-body">
          {actions.map((action) => (
            <button
              key={action.label}
              className="filter-row"
              style={action.danger ? { color: 'var(--red)' } : undefined}
              onClick={() => {
                onClose()
                action.onPress()
              }}
            >
              {action.icon}
              <span className="filter-row-label">{action.label}</span>
            </button>
          ))}
        </div>
      </div>
    </>
  )
}
