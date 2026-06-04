import { useState, useRef, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { IconClose, IconSend, IconSparkles } from './icons'
import { extractErrorMessage } from '../utils'

interface ChatMessage {
  role: 'user' | 'assistant'
  content: string
  bookmarkCount?: number
}

interface AiSearchResponse {
  reply: string
  bookmark_ids: string[]
}

interface Props {
  allBookmarks: Bookmark[]
  folders: { id: string; name: string }[]
  onResults: (ids: string[]) => void
  onClose: () => void
}

export function AiChatPanel({ allBookmarks, folders, onResults, onClose }: Props) {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [input, setInput] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  const folderMap = useRef(new Map(folders.map((f) => [f.id, f.name])))
  useEffect(() => {
    folderMap.current = new Map(folders.map((f) => [f.id, f.name]))
  }, [folders])

  const send = useCallback(async () => {
    const q = input.trim()
    if (!q || loading) return

    setInput('')
    setError(null)
    setMessages((prev) => [...prev, { role: 'user', content: q }])
    setLoading(true)

    try {
      const bookmarkPayload = allBookmarks.map((b) => ({
        id: b.id,
        title: b.title,
        url: b.url,
        description: b.description,
        tags: b.tags.map((t) => t.name),
        folder_name: b.folder_id ? (folderMap.current.get(b.folder_id) ?? null) : null,
      }))

      const res = await invoke<AiSearchResponse>('ai_search', {
        query: q,
        bookmarks: bookmarkPayload,
      })

      setMessages((prev) => [
        ...prev,
        { role: 'assistant', content: res.reply, bookmarkCount: res.bookmark_ids.length },
      ])
      onResults(res.bookmark_ids)
    } catch (e) {
      const msg = extractErrorMessage(e)
      setError(msg)
      setMessages((prev) => [
        ...prev,
        { role: 'assistant', content: `Error: ${msg}` },
      ])
    } finally {
      setLoading(false)
    }
  }, [input, loading, allBookmarks, onResults])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  return (
    <div
      className="flex flex-col flex-none"
      style={{
        width: 320,
        borderLeft: '1px solid var(--border-soft)',
        background: 'var(--bg)',
      }}
    >
      {/* Header */}
      <div
        className="flex items-center justify-between px-4 py-3 flex-none"
        style={{ borderBottom: '1px solid var(--border-soft)', background: 'var(--header-bg)' }}
      >
        <div className="flex items-center gap-2">
          <span style={{ color: 'var(--accent)' }}>
            <IconSparkles size={13} />
          </span>
          <span style={{ fontSize: 13, fontWeight: 600, color: 'var(--text-1)' }}>
            AI Search
          </span>
        </div>
        <button
          onClick={onClose}
          className="flex items-center justify-center rounded transition-colors cursor-pointer"
          style={{ color: 'var(--text-3)', width: 24, height: 24 }}
          onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-1)')}
          onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-3)')}
          aria-label="Close AI search"
        >
          <IconClose size={13} />
        </button>
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto px-4 py-3" style={{ minHeight: 0 }}>
        {messages.length === 0 && (
          <div
            className="flex flex-col items-center justify-center h-full gap-3 text-center"
            style={{ color: 'var(--text-3)', minHeight: 120 }}
          >
            <IconSparkles size={20} />
            <p style={{ fontSize: 12.5, lineHeight: 1.5 }}>
              Describe what you're looking for and I'll find relevant bookmarks.
            </p>
            <p style={{ fontSize: 11, color: 'var(--text-muted)' }}>
              e.g. "React state management", "articles about Rust async"
            </p>
          </div>
        )}

        {messages.map((msg, i) => (
          <div
            key={i}
            className={`mb-3 ${msg.role === 'user' ? 'flex justify-end' : 'flex justify-start'}`}
          >
            <div
              style={{
                maxWidth: '85%',
                padding: '8px 11px',
                borderRadius: msg.role === 'user' ? '12px 12px 3px 12px' : '12px 12px 12px 3px',
                fontSize: 12.5,
                lineHeight: 1.5,
                background: msg.role === 'user' ? 'var(--accent)' : 'var(--bg-elevated)',
                color: msg.role === 'user' ? '#fff' : 'var(--text-1)',
                border: msg.role === 'assistant' ? '1px solid var(--border-soft)' : 'none',
              }}
            >
              <p>{msg.content}</p>
              {msg.role === 'assistant' && msg.bookmarkCount !== undefined && (
                <p
                  style={{
                    marginTop: 5,
                    fontSize: 11,
                    color: msg.bookmarkCount > 0 ? 'var(--accent)' : 'var(--text-3)',
                    fontWeight: 500,
                  }}
                >
                  {msg.bookmarkCount > 0
                    ? `Showing ${msg.bookmarkCount} bookmark${msg.bookmarkCount === 1 ? '' : 's'}`
                    : 'No matching bookmarks'}
                </p>
              )}
            </div>
          </div>
        ))}

        {loading && (
          <div className="flex justify-start mb-3">
            <div
              style={{
                padding: '8px 11px',
                borderRadius: '12px 12px 12px 3px',
                fontSize: 12.5,
                background: 'var(--bg-elevated)',
                border: '1px solid var(--border-soft)',
                color: 'var(--text-3)',
              }}
            >
              <span className="animate-pulse">Searching…</span>
            </div>
          </div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div
        className="flex-none px-3 py-3"
        style={{ borderTop: '1px solid var(--border-soft)' }}
      >
        {error && (
          <p
            className="mb-2 text-xs truncate"
            style={{ color: 'var(--red)' }}
            title={error}
          >
            {error}
          </p>
        )}
        <div
          className="flex items-center gap-2 rounded-lg px-3"
          style={{
            height: 36,
            background: 'var(--input-bg)',
            border: '1px solid var(--border-soft)',
          }}
        >
          <input
            ref={inputRef}
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Ask about your bookmarks…"
            className="bg-transparent flex-1 min-w-0 outline-none"
            style={{ fontSize: 12.5, color: 'var(--text-1)' }}
            disabled={loading}
            aria-label="AI search input"
          />
          <button
            onClick={send}
            disabled={!input.trim() || loading}
            className="flex-none flex items-center justify-center rounded transition-colors cursor-pointer"
            style={{
              width: 24,
              height: 24,
              color: input.trim() && !loading ? 'var(--accent)' : 'var(--text-3)',
              opacity: input.trim() && !loading ? 1 : 0.5,
            }}
            aria-label="Send message"
          >
            <IconSend size={13} />
          </button>
        </div>
      </div>
    </div>
  )
}
