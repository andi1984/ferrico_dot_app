import { useState, useRef, useEffect } from 'react'
import type { Folder, Tag } from '../types'
import { ModalShell, FieldLabel, ModalActions } from './ModalShell'
import { IconChevronDown } from './icons'

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
}

export function AddBookmarkModal({ folders, tags, onAdd, onClose }: AddBookmarkModalProps) {
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
    onAdd({
      url: url.trim(),
      title: title.trim(),
      description: description.trim(),
      folder_id: folderId || null,
      tag_ids: [...selectedTags],
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

        {tags.length > 0 && (
          <div>
            <FieldLabel>Tags</FieldLabel>
            <div className="flex flex-wrap gap-2 pt-1" role="group" aria-label="Select tags">
              {tags.map((tag) => (
                <button
                  key={tag.id}
                  type="button"
                  onClick={() => toggleTag(tag.id)}
                  aria-pressed={selectedTags.has(tag.id)}
                  className="px-2.5 py-1 rounded text-xs font-medium transition-all duration-150 cursor-pointer"
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
