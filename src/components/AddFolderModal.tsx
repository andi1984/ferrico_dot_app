import { useState, useRef, useEffect } from 'react'
import { ModalShell, FieldLabel, ModalActions } from './ModalShell'

export function AddFolderModal({ onAdd, onClose, parentName }: {
  onAdd: (name: string) => void
  onClose: () => void
  // When set, the modal creates a subfolder under this folder.
  parentName?: string
}) {
  const [name, setName] = useState('')
  const ref = useRef<HTMLInputElement>(null)

  useEffect(() => { ref.current?.focus() }, [])

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!name.trim()) return
    onAdd(name.trim())
  }

  const isSubfolder = parentName !== undefined

  return (
    <ModalShell title={isSubfolder ? 'New Subfolder' : 'New Folder'} onClose={onClose}>
      <form onSubmit={handleSubmit} className="p-6 flex flex-col gap-5">
        {isSubfolder && (
          <p style={{ fontSize: 12.5, color: 'var(--text-3)', marginTop: -4 }}>
            in <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>{parentName}</span>
          </p>
        )}
        <div>
          <FieldLabel htmlFor="folder-name">Name</FieldLabel>
          <input id="folder-name" ref={ref} value={name} onChange={(e) => setName(e.target.value)} placeholder="Folder name" className="ff" />
        </div>
        <ModalActions onClose={onClose} submitLabel={isSubfolder ? 'Create subfolder' : 'Create folder'} />
      </form>
    </ModalShell>
  )
}
