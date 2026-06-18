import { useState, useRef, useEffect } from 'react'
import type { Folder, Tag } from '../types'
import { ModalShell, FieldLabel, ModalActions } from './ModalShell'
import { IconChevronDown } from './icons'
import { TagCombobox } from './TagCombobox'

export interface AddBookmarkModalProps {
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
  /** Create a brand-new tag inline (already persisted), returning it. */
  onCreateTag: (name: string, color: string) => Promise<Tag>
  /** Fetch tags co-occurring with the selection for context suggestions. */
  getRelatedTags?: (ids: string[]) => Promise<Tag[]>
}

export function AddBookmarkModal({ folders, tags, onAdd, onClose, onCreateTag, getRelatedTags }: AddBookmarkModalProps) {
  const [url, setUrl] = useState('')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [folderId, setFolderId] = useState('')
  const [selectedTagIds, setSelectedTagIds] = useState<string[]>([])
  const urlRef = useRef<HTMLInputElement>(null)

  useEffect(() => { urlRef.current?.focus() }, [])

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!url.trim() || !title.trim()) return
    onAdd({
      url: url.trim(),
      title: title.trim(),
      description: description.trim(),
      folder_id: folderId || null,
      tag_ids: selectedTagIds,
      feed_url: null,
    })
  }

  return (
    <ModalShell title="New Bookmark" onClose={onClose}>
      <form onSubmit={handleSubmit} className="p-6 flex flex-col gap-5">
        <div>
          <FieldLabel htmlFor="bm-url">URL *</FieldLabel>
          <input id="bm-url" ref={urlRef} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com" className="ff" />
        </div>
        <div>
          <FieldLabel htmlFor="bm-title">Title *</FieldLabel>
          <input id="bm-title" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Page title" className="ff" />
        </div>
        <div>
          <FieldLabel htmlFor="bm-note">Note</FieldLabel>
          <textarea id="bm-note" value={description} onChange={(e) => setDescription(e.target.value)} placeholder="Optional note…" rows={2} className="ff" />
        </div>

        {folders.length > 0 && (
          <div>
            <FieldLabel htmlFor="bm-folder">Folder</FieldLabel>
            <div className="relative">
              <select id="bm-folder" value={folderId} onChange={(e) => setFolderId(e.target.value)} className="ff pr-6">
                <option value="">No folder</option>
                {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
              </select>
              <span className="absolute right-0 top-1/2 -translate-y-1/2 pointer-events-none" style={{ color: 'var(--text-muted)' }}>
                <IconChevronDown size={13} />
              </span>
            </div>
          </div>
        )}

        <div>
          <FieldLabel>Tags</FieldLabel>
          <TagCombobox
            tags={tags}
            selectedIds={selectedTagIds}
            onChange={setSelectedTagIds}
            onCreateTag={onCreateTag}
            getRelatedTags={getRelatedTags}
          />
        </div>

        <ModalActions onClose={onClose} submitLabel="Save bookmark" />
      </form>
    </ModalShell>
  )
}
