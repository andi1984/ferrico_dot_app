import { useState, useRef, useEffect } from 'react'
import { ModalShell, FieldLabel, ModalActions } from './ModalShell'

export function AddFolderModal({ onAdd, onClose }: {
  onAdd: (name: string) => void
  onClose: () => void
}) {
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
          <FieldLabel htmlFor="folder-name">Name</FieldLabel>
          <input id="folder-name" ref={ref} value={name} onChange={(e) => setName(e.target.value)} placeholder="Folder name" className="ff" />
        </div>
        <ModalActions onClose={onClose} submitLabel="Create folder" />
      </form>
    </ModalShell>
  )
}
