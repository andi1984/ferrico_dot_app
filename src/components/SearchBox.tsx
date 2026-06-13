import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react'
import { IconSearch, IconClose } from './icons'

export interface SearchBoxHandle {
  focus: () => void
}

interface SearchBoxProps {
  /**
   * Called with the *debounced* query. The parent only re-renders when this
   * fires (every `debounceMs`), never on individual keystrokes.
   */
  onSearch: (value: string) => void
  debounceMs?: number
}

/**
 * Self-contained search field. The per-keystroke value lives in this small
 * component's local state, so typing re-renders only the input — not the whole
 * App tree (which on a slow machine is what makes typing feel laggy). The parent
 * receives just the debounced query via `onSearch`, and can focus the field
 * through the imperative `focus()` handle (for the ⌘F shortcut).
 */
export const SearchBox = forwardRef<SearchBoxHandle, SearchBoxProps>(function SearchBox(
  { onSearch, debounceMs = 300 },
  ref,
) {
  const [value, setValue] = useState('')
  const [focused, setFocused] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  useImperativeHandle(ref, () => ({ focus: () => inputRef.current?.focus() }), [])

  useEffect(() => {
    const t = setTimeout(() => onSearch(value), debounceMs)
    return () => clearTimeout(t)
  }, [value, debounceMs, onSearch])

  return (
    <div
      className="flex items-center gap-2 rounded-lg px-2.5 min-w-0 transition-colors duration-150"
      style={{
        height: 32,
        maxWidth: 260,
        flex: '1 1 160px',
        minWidth: 120,
        background: 'var(--input-bg)',
        border: `1px solid ${focused ? 'var(--accent)' : 'var(--border-soft)'}`,
        boxShadow: focused ? '0 0 0 2px var(--accent-glow)' : 'none',
      }}
    >
      <span className="flex-none" style={{ color: focused ? 'var(--accent)' : 'var(--text-3)' }} aria-hidden="true">
        <IconSearch size={13} />
      </span>
      <input
        ref={inputRef}
        type="search"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onKeyDown={(e) => {
          if (e.key === 'Escape' && value) {
            e.stopPropagation()
            setValue('')
          }
        }}
        placeholder="Search bookmarks…"
        aria-label="Search bookmarks"
        className="bg-transparent flex-1 min-w-0 outline-none"
        style={{ color: 'var(--text-1)', fontSize: 12.5 }}
      />
      {value && (
        <button
          onClick={() => {
            setValue('')
            inputRef.current?.focus()
          }}
          className="flex-none transition-colors duration-150 cursor-pointer"
          style={{ color: 'var(--text-3)' }}
          onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-2)')}
          onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-3)')}
          aria-label="Clear search"
        >
          <IconClose size={11} />
        </button>
      )}
      {!value && (
        <span
          className="mono shrink-0"
          style={{
            fontSize: 10,
            color: 'var(--text-3)',
            padding: '1px 5px',
            border: '1px solid var(--border-soft)',
            borderRadius: 4,
          }}
          aria-hidden="true"
        >⌘F</span>
      )}
    </div>
  )
})
