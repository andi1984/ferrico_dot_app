import { useState, useRef, useEffect, useMemo, useCallback } from 'react'
import type { Tag } from '../types'
import { IconPlus, IconCheck } from './icons'
import { TAG_COLORS } from './AddTagModal'

export interface TagComboboxProps {
  /** All tags known to the app (used to resolve chips + power autocomplete). */
  tags: Tag[]
  /** Currently selected tag ids. */
  selectedIds: string[]
  onChange: (ids: string[]) => void
  /** Create a brand-new tag and return it (already persisted). */
  onCreateTag: (name: string, color: string) => Promise<Tag>
  /** Fetch tags that co-occur with the given selection (context suggestions). */
  getRelatedTags?: (ids: string[]) => Promise<Tag[]>
}

/** Stable color pick for one-click "new tag" suggestions, mirroring the extension. */
function hashColor(str: string): string {
  let h = 0
  for (let i = 0; i < str.length; i++) h = (h * 31 + str.charCodeAt(i)) >>> 0
  return TAG_COLORS[h % TAG_COLORS.length]
}

/**
 * Chip-based tag picker with autocomplete + context-aware suggestions, ported
 * from the browser extension's `mountTagCombobox`. Replaces the old wall of
 * toggle buttons that overflowed once a user had many tags.
 */
export function TagCombobox({ tags, selectedIds, onChange, onCreateTag, getRelatedTags }: TagComboboxProps) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const [activeIndex, setActiveIndex] = useState(-1)
  // Tags created inline (or returned by related lookups) that may not be in the
  // parent `tags` prop yet — merged so chips/labels resolve immediately.
  const [extraTags, setExtraTags] = useState<Tag[]>([])
  const [related, setRelated] = useState<Tag[]>([])

  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const allTags = useMemo(() => {
    const seen = new Set(tags.map((t) => t.id))
    return [...tags, ...extraTags.filter((t) => !seen.has(t.id))]
  }, [tags, extraTags])

  const tagById = useCallback((id: string) => allTags.find((t) => t.id === id), [allTags])

  const q = query.toLowerCase().trim()

  const filtered = useMemo(
    () =>
      allTags
        .filter((t) => t.name.toLowerCase().includes(q))
        .sort((a, b) => a.name.localeCompare(b.name)),
    [allTags, q],
  )

  const hasExactMatch = q !== '' && allTags.some((t) => t.name.toLowerCase() === q)
  const showCreate = q !== '' && !hasExactMatch
  const totalItems = filtered.length + (showCreate ? 1 : 0)

  // ── Context suggestions: tags co-occurring with the current selection ──
  useEffect(() => {
    if (!getRelatedTags || selectedIds.length === 0) {
      setRelated([])
      return
    }
    let cancelled = false
    getRelatedTags([...selectedIds])
      .then((r) => {
        if (cancelled) return
        setRelated(Array.isArray(r) ? r : [])
        // Make sure suggested tags can be rendered as chips when picked.
        setExtraTags((prev) => {
          const seen = new Set([...tags, ...prev].map((t) => t.id))
          const add = r.filter((t) => !seen.has(t.id))
          return add.length ? [...prev, ...add] : prev
        })
      })
      .catch(() => { if (!cancelled) setRelated([]) })
    return () => { cancelled = true }
  }, [selectedIds, getRelatedTags, tags])

  const suggestions = useMemo(
    () => related.filter((t) => !selectedIds.includes(t.id)).slice(0, 8),
    [related, selectedIds],
  )

  // ── Selection helpers ──
  function toggle(id: string) {
    onChange(selectedIds.includes(id) ? selectedIds.filter((x) => x !== id) : [...selectedIds, id])
    inputRef.current?.focus()
  }

  function add(id: string) {
    if (!selectedIds.includes(id)) onChange([...selectedIds, id])
  }

  function remove(id: string) {
    onChange(selectedIds.filter((x) => x !== id))
  }

  async function createAndAdd(name: string) {
    const trimmed = name.trim()
    if (!trimmed) return
    const existing = allTags.find((t) => t.name.toLowerCase() === trimmed.toLowerCase())
    if (existing) { add(existing.id); }
    else {
      try {
        const tag = await onCreateTag(trimmed, hashColor(trimmed))
        setExtraTags((prev) => (prev.some((t) => t.id === tag.id) ? prev : [...prev, tag]))
        onChange([...selectedIds, tag.id])
      } catch {
        return
      }
    }
    setQuery('')
    setActiveIndex(-1)
    inputRef.current?.focus()
  }

  // ── Keyboard ──
  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setOpen(true)
      setActiveIndex((i) => Math.min(i + 1, totalItems - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActiveIndex((i) => Math.max(i - 1, -1))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (activeIndex >= 0 && activeIndex < filtered.length) {
        toggle(filtered[activeIndex].id)
      } else if (showCreate && activeIndex === filtered.length) {
        createAndAdd(query)
      } else if (filtered.length > 0) {
        toggle(filtered[0].id)
      } else if (showCreate) {
        createAndAdd(query)
      }
    } else if (e.key === 'Escape') {
      if (open) { e.stopPropagation(); setOpen(false) }
    } else if (e.key === 'Backspace' && query === '' && selectedIds.length > 0) {
      remove(selectedIds[selectedIds.length - 1])
    }
  }

  // Keep the active option scrolled into view.
  useEffect(() => {
    if (!open) return
    const el = listRef.current?.querySelector('[data-active="true"]')
    el?.scrollIntoView({ block: 'nearest' })
  }, [activeIndex, open])

  return (
    <div className="relative">
      {/* Pill input */}
      <div
        className="flex flex-wrap items-center gap-1.5 rounded-lg px-2 py-1.5 cursor-text transition-colors duration-150"
        style={{
          background: 'var(--input-bg)',
          border: `1px solid ${open ? 'var(--accent)' : 'var(--border-soft)'}`,
          boxShadow: open ? '0 0 0 3px var(--accent-glow)' : 'none',
        }}
        onMouseDown={(e) => {
          if (e.target === e.currentTarget) { e.preventDefault(); inputRef.current?.focus() }
        }}
      >
        {selectedIds.map((id) => {
          const tag = tagById(id)
          if (!tag) return null
          return (
            <span
              key={id}
              className="inline-flex items-center gap-1.5 rounded px-2 py-0.5 text-xs font-medium"
              style={{ background: tag.color + '28', color: tag.color, border: `1px solid ${tag.color}66` }}
            >
              <span className="inline-block w-1.5 h-1.5 rounded-full" style={{ background: tag.color }} />
              {tag.name}
              <button
                type="button"
                aria-label={`Remove ${tag.name}`}
                onClick={() => remove(id)}
                className="ml-0.5 leading-none cursor-pointer opacity-70 hover:opacity-100"
                style={{ color: tag.color }}
              >
                ×
              </button>
            </span>
          )
        })}
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => { setQuery(e.target.value); setActiveIndex(-1); setOpen(true) }}
          onFocus={() => setOpen(true)}
          onBlur={() => setTimeout(() => setOpen(false), 120)}
          onKeyDown={onKeyDown}
          placeholder={selectedIds.length === 0 ? 'Search or add tags…' : ''}
          autoComplete="off"
          spellCheck={false}
          className="flex-1 min-w-[8rem] bg-transparent outline-none text-sm py-0.5"
          style={{ color: 'var(--text-1)' }}
          role="combobox"
          aria-expanded={open}
          aria-autocomplete="list"
        />
      </div>

      {/* Autocomplete dropdown */}
      {open && (
        <div
          ref={listRef}
          className="absolute z-20 left-0 right-0 mt-1 rounded-lg overflow-auto py-1 shadow-lg"
          style={{
            maxHeight: 220,
            background: 'var(--bg-elev-strong)',
            border: '1px solid var(--border-soft)',
          }}
          role="listbox"
        >
          {filtered.length === 0 && !showCreate && (
            <div className="px-3 py-2 text-xs" style={{ color: 'var(--text-muted)' }}>
              {q ? 'No matching tags' : 'No tags yet'}
            </div>
          )}

          {filtered.map((tag, i) => {
            const isSelected = selectedIds.includes(tag.id)
            const isActive = i === activeIndex
            return (
              <div
                key={tag.id}
                data-active={isActive}
                role="option"
                aria-selected={isSelected}
                onMouseDown={(e) => { e.preventDefault(); toggle(tag.id) }}
                onMouseEnter={() => setActiveIndex(i)}
                className="flex items-center gap-2 px-3 py-1.5 text-sm cursor-pointer"
                style={{ background: isActive ? 'var(--btn-hover-bg)' : 'transparent', color: 'var(--text-1)' }}
              >
                <span className="inline-block w-2 h-2 rounded-full shrink-0" style={{ background: tag.color }} />
                <span className="flex-1 truncate">{tag.name}</span>
                {tag.bookmark_count != null && (
                  <span className="text-xs tabular-nums" style={{ color: 'var(--text-muted)' }}>{tag.bookmark_count}</span>
                )}
                {isSelected && (
                  <span style={{ color: 'var(--accent)' }}><IconCheck size={13} /></span>
                )}
              </div>
            )
          })}

          {showCreate && (
            <>
              {filtered.length > 0 && <div className="my-1 h-px" style={{ background: 'var(--border-soft)' }} />}
              <div
                data-active={activeIndex === filtered.length}
                role="option"
                aria-selected={false}
                onMouseDown={(e) => { e.preventDefault(); createAndAdd(query) }}
                onMouseEnter={() => setActiveIndex(filtered.length)}
                className="flex items-center gap-2 px-3 py-1.5 text-sm cursor-pointer"
                style={{ background: activeIndex === filtered.length ? 'var(--btn-hover-bg)' : 'transparent', color: 'var(--accent)' }}
              >
                <IconPlus size={13} />
                <span className="truncate">Create “<strong>{query.trim()}</strong>”</span>
              </div>
            </>
          )}
        </div>
      )}

      {/* Context suggestions */}
      {suggestions.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 pt-2.5">
          <span className="text-xs font-medium uppercase tracking-wider mr-0.5" style={{ color: 'var(--text-muted)' }}>
            Suggested
          </span>
          {suggestions.map((tag) => (
            <button
              key={tag.id}
              type="button"
              onClick={() => add(tag.id)}
              title={`Add tag “${tag.name}”`}
              className="inline-flex items-center gap-1.5 rounded px-2 py-0.5 text-xs font-medium cursor-pointer transition-colors duration-150"
              style={{ background: 'transparent', color: 'var(--text-2)', border: '1px solid var(--border-soft)' }}
            >
              <span className="inline-block w-1.5 h-1.5 rounded-full" style={{ background: tag.color }} />
              {tag.name}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
