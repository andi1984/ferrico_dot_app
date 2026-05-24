import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark, Folder } from '../types'
import { IconClose, IconSparkles } from './icons'

interface SortSuggestion {
  bookmark_id: string
  folder_name: string
}

interface SortResult {
  moved: number
}

interface Assignment {
  bookmark_id: string
  folder_name: string
  accepted: boolean
}

type Step = 'analyzing' | 'review' | 'applying' | 'done'

export interface InboxSortModalProps {
  bookmarks: Bookmark[]
  folders: Folder[]
  onClose: () => void
  onDone: () => void
}

export function InboxSortModal({ bookmarks, folders, onClose, onDone }: InboxSortModalProps) {
  const [step, setStep] = useState<Step>('analyzing')
  const [assignments, setAssignments] = useState<Assignment[]>([])
  const [claudeError, setClaudeError] = useState<string | null>(null)
  const [result, setResult] = useState<SortResult | null>(null)
  const [applyError, setApplyError] = useState<string | null>(null)

  const folderNames = folders.map((f) => f.name)

  useEffect(() => {
    const inputs = bookmarks.map((b) => ({
      id: b.id,
      url: b.url,
      title: b.title,
      description: b.description ?? null,
    }))

    invoke<SortSuggestion[]>('suggest_inbox_sort', {
      bookmarks: inputs,
      folderNames,
    })
      .then((suggestions) => {
        const map = new Map(suggestions.map((s) => [s.bookmark_id, s.folder_name]))
        const built: Assignment[] = bookmarks.map((b) => ({
          bookmark_id: b.id,
          folder_name: map.get(b.id) ?? folderNames[0] ?? '',
          accepted: map.has(b.id),
        }))
        setAssignments(built)
        setStep('review')
      })
      .catch((e) => {
        const msg = e instanceof Error ? e.message : String(e)
        setClaudeError(msg)
        const built: Assignment[] = bookmarks.map((b) => ({
          bookmark_id: b.id,
          folder_name: folderNames[0] ?? '',
          accepted: false,
        }))
        setAssignments(built)
        setStep('review')
      })
  // Run once on mount
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function handleApply() {
    const toMove = assignments.filter((a) => a.accepted && a.folder_name.trim())
    if (toMove.length === 0) {
      onClose()
      return
    }
    setStep('applying')
    setApplyError(null)
    try {
      const r = await invoke<SortResult>('apply_inbox_sort', {
        assignments: toMove.map((a) => ({
          bookmark_id: a.bookmark_id,
          folder_name: a.folder_name.trim(),
        })),
      })
      setResult(r)
      setStep('done')
      onDone()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setApplyError(msg)
      setStep('review')
    }
  }

  function toggleAccepted(id: string) {
    setAssignments((prev) =>
      prev.map((a) => (a.bookmark_id === id ? { ...a, accepted: !a.accepted } : a))
    )
  }

  function setFolder(id: string, name: string) {
    setAssignments((prev) =>
      prev.map((a) => (a.bookmark_id === id ? { ...a, folder_name: name } : a))
    )
  }

  function toggleAll(val: boolean) {
    setAssignments((prev) => prev.map((a) => ({ ...a, accepted: val })))
  }

  const acceptedCount = assignments.filter((a) => a.accepted && a.folder_name.trim()).length
  const allAccepted = assignments.length > 0 && assignments.every((a) => a.accepted)
  const noneAccepted = assignments.every((a) => !a.accepted)

  const stepTitle: Record<Step, string> = {
    analyzing: 'AI Sorting',
    review: 'Review Suggestions',
    applying: 'Applying…',
    done: 'Sorting Complete',
  }

  const bookmarkById = new Map(bookmarks.map((b) => [b.id, b]))

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      style={{ background: 'rgba(0,0,0,0.75)', backdropFilter: 'blur(4px)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget && step !== 'applying') onClose()
      }}
    >
      <div
        className="anim-scale-in w-full rounded-xl shadow-2xl overflow-hidden flex flex-col"
        style={{ background: 'var(--bg-surface)', border: '1px solid var(--border-mid)', maxWidth: '42rem', maxHeight: '85vh' }}
        role="dialog"
        aria-modal="true"
        aria-label={stepTitle[step]}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 flex-none" style={{ borderBottom: '1px solid var(--border-dim)' }}>
          <div className="flex items-center gap-2">
            <span style={{ color: 'var(--accent)' }} aria-hidden="true">
              <IconSparkles size={15} />
            </span>
            <span className="text-sm font-semibold tracking-wide" style={{ color: 'var(--text-primary)' }}>
              {stepTitle[step]}
            </span>
          </div>
          {step !== 'applying' && (
            <button
              onClick={onClose}
              className="rounded p-1 transition-colors duration-150 cursor-pointer"
              style={{ color: 'var(--text-muted)' }}
              onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-primary)')}
              onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
              aria-label="Close dialog"
            >
              <IconClose size={15} />
            </button>
          )}
        </div>

        {/* Step: analyzing */}
        {step === 'analyzing' && (
          <div className="p-10 flex flex-col items-center gap-5">
            <span
              className="inline-block w-8 h-8 rounded-full border-2 animate-spin flex-none"
              style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
            />
            <div className="text-center">
              <p className="text-sm font-medium" style={{ color: 'var(--text-primary)' }}>
                Analyzing {bookmarks.length} bookmark{bookmarks.length === 1 ? '' : 's'}…
              </p>
              <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>
                Claude is suggesting folders for each bookmark
              </p>
            </div>
          </div>
        )}

        {/* Step: review */}
        {step === 'review' && (
          <>
            <div className="flex-1 overflow-y-auto">
              {claudeError && (
                <div className="mx-6 mt-5 rounded-lg px-4 py-3 text-xs" style={{ background: 'rgba(224,82,82,0.1)', color: '#e07070', border: '1px solid rgba(224,82,82,0.2)' }}>
                  Claude is unavailable — you can still assign folders manually.
                </div>
              )}
              {applyError && (
                <div className="mx-6 mt-5 rounded-lg px-4 py-3 text-xs" style={{ background: 'rgba(224,82,82,0.1)', color: '#e07070', border: '1px solid rgba(224,82,82,0.2)' }}>
                  {applyError}
                </div>
              )}

              <div className="px-6 pt-5 pb-2 flex items-center justify-between">
                <p className="text-xs uppercase tracking-widest font-medium" style={{ color: 'var(--text-muted)' }}>
                  {bookmarks.length} inbox bookmark{bookmarks.length === 1 ? '' : 's'}
                </p>
                <div className="flex items-center gap-3">
                  <button
                    onClick={() => toggleAll(true)}
                    disabled={allAccepted}
                    className="text-xs transition-colors duration-150 cursor-pointer"
                    style={{ color: allAccepted ? 'var(--text-muted)' : 'var(--accent)' }}
                  >
                    Accept all
                  </button>
                  <button
                    onClick={() => toggleAll(false)}
                    disabled={noneAccepted}
                    className="text-xs transition-colors duration-150 cursor-pointer"
                    style={{ color: noneAccepted ? 'var(--text-muted)' : 'var(--text-secondary)' }}
                  >
                    Clear all
                  </button>
                </div>
              </div>

              {/* datalist for folder suggestions */}
              <datalist id="inbox-sort-folders">
                {folderNames.map((name) => (
                  <option key={name} value={name} />
                ))}
              </datalist>

              <div className="px-6 pb-4 flex flex-col gap-2">
                {assignments.map((a) => {
                  const b = bookmarkById.get(a.bookmark_id)
                  if (!b) return null
                  return (
                    <div
                      key={a.bookmark_id}
                      className="flex items-center gap-3 rounded-lg px-3 py-2.5 transition-colors duration-100"
                      style={{
                        background: a.accepted ? 'rgba(200,160,90,0.06)' : 'var(--bg-elevated)',
                        border: `1px solid ${a.accepted ? 'rgba(200,160,90,0.18)' : 'var(--border-dim)'}`,
                      }}
                    >
                      {/* Checkbox */}
                      <input
                        type="checkbox"
                        checked={a.accepted}
                        onChange={() => toggleAccepted(a.bookmark_id)}
                        className="flex-none cursor-pointer"
                        style={{ accentColor: 'var(--accent)', width: 14, height: 14 }}
                        aria-label={`Accept suggestion for ${b.title}`}
                      />

                      {/* Bookmark info */}
                      <div className="flex-1 min-w-0">
                        <p
                          className="text-sm font-medium truncate"
                          style={{ color: a.accepted ? 'var(--text-primary)' : 'var(--text-secondary)' }}
                        >
                          {b.title}
                        </p>
                        <p className="text-xs truncate" style={{ color: 'var(--text-muted)' }}>
                          {b.url}
                        </p>
                      </div>

                      {/* Folder input */}
                      <div className="flex items-center gap-1.5 flex-none">
                        <span className="text-xs" style={{ color: 'var(--text-muted)' }}>→</span>
                        <input
                          type="text"
                          list="inbox-sort-folders"
                          value={a.folder_name}
                          onChange={(e) => setFolder(a.bookmark_id, e.target.value)}
                          placeholder="Folder name…"
                          className="text-xs rounded px-2 py-1 outline-none transition-colors duration-100"
                          style={{
                            width: '130px',
                            background: 'var(--bg-base)',
                            border: '1px solid var(--border-dim)',
                            color: 'var(--text-primary)',
                          }}
                          onFocus={(e) => (e.currentTarget.style.borderColor = 'var(--accent)')}
                          onBlur={(e) => (e.currentTarget.style.borderColor = 'var(--border-dim)')}
                          aria-label={`Folder for ${b.title}`}
                        />
                      </div>
                    </div>
                  )
                })}
              </div>
            </div>

            {/* Footer actions */}
            <div className="flex-none px-6 py-4 flex gap-2" style={{ borderTop: '1px solid var(--border-dim)' }}>
              <button
                type="button"
                onClick={onClose}
                className="flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-colors duration-150 cursor-pointer"
                style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
                onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
                onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleApply}
                className="flex-1 px-4 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 cursor-pointer"
                style={{ background: 'var(--accent)', color: '#0c0b0a', opacity: acceptedCount > 0 ? 1 : 0.45 }}
              >
                {acceptedCount > 0
                  ? `Move ${acceptedCount} bookmark${acceptedCount === 1 ? '' : 's'}`
                  : 'No bookmarks selected'}
              </button>
            </div>
          </>
        )}

        {/* Step: applying */}
        {step === 'applying' && (
          <div className="p-10 flex flex-col items-center gap-5">
            <span
              className="inline-block w-8 h-8 rounded-full border-2 animate-spin flex-none"
              style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
            />
            <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
              Moving bookmarks…
            </p>
          </div>
        )}

        {/* Step: done */}
        {step === 'done' && result && (
          <div className="p-6 flex flex-col gap-5">
            <div className="flex flex-col items-center gap-3 py-4">
              <div
                className="w-12 h-12 rounded-full flex items-center justify-center"
                style={{ background: 'rgba(200,160,90,0.12)' }}
                aria-hidden="true"
              >
                <IconSparkles size={20} />
              </div>
              <p className="text-sm font-medium text-center" style={{ color: 'var(--text-primary)' }}>
                {result.moved === 0
                  ? 'Nothing to move.'
                  : `Sorted ${result.moved} bookmark${result.moved === 1 ? '' : 's'} into folders.`}
              </p>
            </div>
            <button
              onClick={onClose}
              className="w-full px-4 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 hover:opacity-90 cursor-pointer"
              style={{ background: 'var(--accent)', color: '#0c0b0a' }}
            >
              Done
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
