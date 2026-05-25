import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { Bookmark } from '../types'
import { IconClose, IconSparkles, IconCheck, IconLayers } from './icons'

interface DuplicateGroup {
  bookmarks: Bookmark[]
  keeperId: string
}

interface MergeInput {
  keeper_id: string
  discard_ids: string[]
}

type Step = 'scanning' | 'empty' | 'review' | 'merging' | 'done'

export interface DeduplicateModalProps {
  onClose: () => void
  onDone: () => void
}

function scoreBookmark(b: Bookmark): number {
  return (b.description ? 10 : 0) + b.tags.length * 2 + (b.folder_id ? 3 : 0)
}

function pickDefaultKeeper(bookmarks: Bookmark[]): string {
  const best = bookmarks.reduce((a, b) => {
    const sa = scoreBookmark(a), sb = scoreBookmark(b)
    if (sa !== sb) return sa > sb ? a : b
    return a.updated_at >= b.updated_at ? a : b
  })
  return best.id
}

function domainOf(url: string): string {
  try { return new URL(url).hostname } catch { return url }
}

function formatDate(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

export function DeduplicateModal({ onClose, onDone }: DeduplicateModalProps) {
  const [step, setStep] = useState<Step>('scanning')
  const [groups, setGroups] = useState<DuplicateGroup[]>([])
  const [currentGroupIdx, setCurrentGroupIdx] = useState(0)
  const [mergedCount, setMergedCount] = useState(0)
  const [claudeLoading, setClaudeLoading] = useState(false)
  const [claudeError, setClaudeError] = useState<string | null>(null)
  const [applyError, setApplyError] = useState<string | null>(null)

  useEffect(() => {
    invoke<Bookmark[][]>('find_duplicate_bookmarks')
      .then((rawGroups) => {
        if (rawGroups.length === 0) {
          setStep('empty')
          return
        }
        const built: DuplicateGroup[] = rawGroups.map((bookmarks) => ({
          bookmarks,
          keeperId: pickDefaultKeeper(bookmarks),
        }))
        setGroups(built)
        setStep('review')
      })
      .catch(() => {
        setGroups([])
        setStep('empty')
      })
  }, [])

  const setKeeper = useCallback((groupIdx: number, bookmarkId: string) => {
    setGroups((prev) =>
      prev.map((g, i) => (i === groupIdx ? { ...g, keeperId: bookmarkId } : g))
    )
  }, [])

  async function handleAutoPickWithClaude() {
    setClaudeLoading(true)
    setClaudeError(null)
    try {
      const input = groups.map((g, i) => ({
        group_index: i,
        bookmarks: g.bookmarks.map((b) => ({
          id: b.id,
          url: b.url,
          title: b.title,
          description: b.description ?? null,
        })),
      }))
      const resolutions = await invoke<{ group_index: number; keeper_id: string }[]>(
        'suggest_duplicate_resolution',
        { groups: input }
      )
      setGroups((prev) =>
        prev.map((g, i) => {
          const res = resolutions.find((r) => r.group_index === i)
          if (!res) return g
          const valid = g.bookmarks.some((b) => b.id === res.keeper_id)
          return valid ? { ...g, keeperId: res.keeper_id } : g
        })
      )
    } catch (e) {
      const raw = e instanceof Error ? e.message : typeof e === 'object' && e !== null && 'message' in e ? (e as { message: string }).message : String(e)
      setClaudeError(raw)
    } finally {
      setClaudeLoading(false)
    }
  }

  async function handleMergeAll() {
    setStep('merging')
    setApplyError(null)
    let count = 0
    try {
      for (const group of groups) {
        const discardIds = group.bookmarks
          .filter((b) => b.id !== group.keeperId)
          .map((b) => b.id)
        if (discardIds.length === 0) continue
        const input: MergeInput = { keeper_id: group.keeperId, discard_ids: discardIds }
        await invoke('merge_bookmark_duplicates', { input })
        count += discardIds.length
      }
      setMergedCount(count)
      setStep('done')
      onDone()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setApplyError(msg)
      setStep('review')
    }
  }

  const stepTitle: Record<Step, string> = {
    scanning: 'Find Duplicates',
    empty: 'Find Duplicates',
    review: 'Remove Duplicates',
    merging: 'Removing…',
    done: 'Done',
  }

  const currentGroup = groups[currentGroupIdx]
  const resolvedCount = groups.filter((g) => g.keeperId !== '').length

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      style={{ background: 'rgba(0,0,0,0.75)', backdropFilter: 'blur(4px)' }}
      onClick={(e) => {
        if (e.target === e.currentTarget && step !== 'merging') onClose()
      }}
    >
      <div
        className="anim-scale-in w-full rounded-xl shadow-2xl overflow-hidden flex flex-col"
        style={{
          background: 'var(--bg-surface)',
          border: '1px solid var(--border-mid)',
          maxWidth: '52rem',
          maxHeight: '88vh',
        }}
        role="dialog"
        aria-modal="true"
        aria-label={stepTitle[step]}
      >
        {/* Header */}
        <div
          className="flex items-center justify-between px-6 py-4 flex-none"
          style={{ borderBottom: '1px solid var(--border-dim)' }}
        >
          <div className="flex items-center gap-2">
            <span style={{ color: 'var(--accent)' }} aria-hidden="true">
              <IconLayers size={15} />
            </span>
            <span className="text-sm font-semibold tracking-wide" style={{ color: 'var(--text-primary)' }}>
              {stepTitle[step]}
            </span>
            {step === 'review' && groups.length > 0 && (
              <span
                className="ml-1 text-xs px-2 py-0.5 rounded-full font-medium"
                style={{ background: 'rgba(200,160,90,0.12)', color: 'var(--accent)' }}
              >
                {groups.length} group{groups.length === 1 ? '' : 's'}
              </span>
            )}
          </div>
          {step !== 'merging' && (
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

        {/* scanning */}
        {step === 'scanning' && (
          <div className="p-10 flex flex-col items-center gap-5">
            <span
              className="inline-block w-8 h-8 rounded-full border-2 animate-spin flex-none"
              style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
            />
            <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
              Scanning for duplicate URLs…
            </p>
          </div>
        )}

        {/* empty */}
        {step === 'empty' && (
          <div className="p-10 flex flex-col items-center gap-4">
            <div
              className="w-12 h-12 rounded-full flex items-center justify-center"
              style={{ background: 'rgba(200,160,90,0.12)' }}
            >
              <IconCheck size={22} />
            </div>
            <div className="text-center">
              <p className="text-sm font-medium" style={{ color: 'var(--text-primary)' }}>
                No duplicates found
              </p>
              <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>
                All bookmark URLs are unique.
              </p>
            </div>
            <button
              onClick={onClose}
              className="mt-2 px-6 py-2 rounded-lg text-sm font-semibold cursor-pointer transition-opacity duration-150 hover:opacity-90"
              style={{ background: 'var(--accent)', color: '#0c0b0a' }}
            >
              Close
            </button>
          </div>
        )}

        {/* review */}
        {step === 'review' && currentGroup && (
          <>
            {/* Toolbar */}
            <div
              className="flex items-center justify-between px-6 py-3 flex-none gap-3"
              style={{ borderBottom: '1px solid var(--border-dim)', background: 'var(--bg-elevated)' }}
            >
              {/* Group nav */}
              <div className="flex items-center gap-2">
                <button
                  onClick={() => setCurrentGroupIdx((i) => Math.max(0, i - 1))}
                  disabled={currentGroupIdx === 0}
                  className="px-2.5 py-1 rounded text-xs font-medium transition-colors duration-100 cursor-pointer disabled:opacity-30"
                  style={{
                    background: 'var(--input-bg)',
                    border: '1px solid var(--border-dim)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  ← Prev
                </button>
                <span className="text-xs font-medium tabnum" style={{ color: 'var(--text-muted)', minWidth: 64, textAlign: 'center' }}>
                  {currentGroupIdx + 1} / {groups.length}
                </span>
                <button
                  onClick={() => setCurrentGroupIdx((i) => Math.min(groups.length - 1, i + 1))}
                  disabled={currentGroupIdx === groups.length - 1}
                  className="px-2.5 py-1 rounded text-xs font-medium transition-colors duration-100 cursor-pointer disabled:opacity-30"
                  style={{
                    background: 'var(--input-bg)',
                    border: '1px solid var(--border-dim)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  Next →
                </button>
              </div>

              {/* AI button */}
              <button
                onClick={handleAutoPickWithClaude}
                disabled={claudeLoading}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all duration-150 cursor-pointer disabled:opacity-50"
                style={{
                  background: 'rgba(200,160,90,0.1)',
                  border: '1px solid rgba(200,160,90,0.25)',
                  color: 'var(--accent)',
                }}
                onMouseEnter={(e) => { if (!claudeLoading) e.currentTarget.style.background = 'rgba(200,160,90,0.18)' }}
                onMouseLeave={(e) => { e.currentTarget.style.background = 'rgba(200,160,90,0.1)' }}
                title="Use Claude to auto-pick the best bookmark in each group"
              >
                {claudeLoading
                  ? <span className="inline-block w-3 h-3 rounded-full border animate-spin flex-none" style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }} />
                  : <IconSparkles size={12} />
                }
                Auto-pick all with AI
              </button>
            </div>

            {claudeError && (
              <div className="mx-6 mt-4 rounded-lg px-4 py-2.5 text-xs flex flex-col gap-1" style={{ background: 'rgba(224,82,82,0.08)', color: '#e07070', border: '1px solid rgba(224,82,82,0.15)' }}>
                <span className="font-medium">Claude unavailable — pick manually below.</span>
                <span className="opacity-70 font-mono break-all">{claudeError}</span>
              </div>
            )}
            {applyError && (
              <div className="mx-6 mt-4 rounded-lg px-4 py-2.5 text-xs" style={{ background: 'rgba(224,82,82,0.08)', color: '#e07070', border: '1px solid rgba(224,82,82,0.15)' }}>
                {applyError}
              </div>
            )}

            {/* Group detail */}
            <div className="flex-1 overflow-y-auto px-6 py-5">
              {/* Shared URL */}
              <div className="mb-4">
                <p className="text-xs uppercase tracking-widest font-medium mb-1.5" style={{ color: 'var(--text-muted)' }}>
                  Shared URL
                </p>
                <p
                  className="text-xs font-mono truncate px-3 py-2 rounded-lg"
                  style={{ background: 'var(--bg-elevated)', border: '1px solid var(--border-dim)', color: 'var(--text-secondary)' }}
                >
                  {domainOf(currentGroup.bookmarks[0]?.url ?? '')} — {currentGroup.bookmarks[0]?.url}
                </p>
              </div>

              <p className="text-xs uppercase tracking-widest font-medium mb-3" style={{ color: 'var(--text-muted)' }}>
                Pick one to keep — the rest go to bin
              </p>

              {/* Bookmark cards */}
              <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))' }}>
                {currentGroup.bookmarks.map((b) => {
                  const isKeeper = b.id === currentGroup.keeperId
                  return (
                    <button
                      key={b.id}
                      onClick={() => setKeeper(currentGroupIdx, b.id)}
                      className="text-left rounded-xl p-4 flex flex-col gap-2.5 transition-all duration-150 cursor-pointer"
                      style={{
                        background: isKeeper ? 'rgba(200,160,90,0.08)' : 'var(--bg-elevated)',
                        border: `2px solid ${isKeeper ? 'var(--accent)' : 'var(--border-dim)'}`,
                        outline: 'none',
                      }}
                      onMouseEnter={(e) => { if (!isKeeper) e.currentTarget.style.borderColor = 'var(--border-bright)' }}
                      onMouseLeave={(e) => { if (!isKeeper) e.currentTarget.style.borderColor = 'var(--border-dim)' }}
                      aria-pressed={isKeeper}
                      aria-label={`Keep "${b.title}"`}
                    >
                      {/* Status badge */}
                      <div className="flex items-center justify-between gap-2">
                        <span
                          className="text-xs font-semibold px-2 py-0.5 rounded-full flex items-center gap-1"
                          style={{
                            background: isKeeper ? 'rgba(200,160,90,0.15)' : 'var(--bg-base)',
                            color: isKeeper ? 'var(--accent)' : 'var(--text-muted)',
                            border: `1px solid ${isKeeper ? 'rgba(200,160,90,0.3)' : 'var(--border-dim)'}`,
                          }}
                        >
                          {isKeeper && <IconCheck size={10} />}
                          {isKeeper ? 'Keeping' : 'Keep this'}
                        </span>
                      </div>

                      {/* Title */}
                      <div>
                        <p
                          className="text-sm font-semibold leading-snug"
                          style={{ color: isKeeper ? 'var(--text-primary)' : 'var(--text-secondary)' }}
                        >
                          {b.title}
                        </p>
                      </div>

                      {/* Metadata chips */}
                      <div className="flex flex-col gap-1.5 text-xs" style={{ color: 'var(--text-muted)' }}>
                        {b.description && (
                          <span
                            className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full w-fit"
                            style={{ background: 'rgba(129,178,154,0.12)', color: '#81b29a', border: '1px solid rgba(129,178,154,0.2)' }}
                          >
                            Has description
                          </span>
                        )}
                        {b.tags.length > 0 && (
                          <div className="flex flex-wrap gap-1">
                            {b.tags.slice(0, 3).map((t) => (
                              <span
                                key={t.id}
                                className="px-1.5 py-0.5 rounded text-xs"
                                style={{ background: `${t.color}20`, color: t.color, border: `1px solid ${t.color}30` }}
                              >
                                {t.name}
                              </span>
                            ))}
                            {b.tags.length > 3 && <span style={{ color: 'var(--text-muted)' }}>+{b.tags.length - 3}</span>}
                          </div>
                        )}
                        <span className="tabnum">{formatDate(b.created_at)}</span>
                      </div>
                    </button>
                  )
                })}
              </div>

              {/* Group dots */}
              {groups.length > 1 && (
                <div className="flex justify-center gap-1.5 mt-6">
                  {groups.map((g, i) => (
                    <button
                      key={i}
                      onClick={() => setCurrentGroupIdx(i)}
                      className="rounded-full transition-all duration-150 cursor-pointer"
                      style={{
                        width: i === currentGroupIdx ? 20 : 8,
                        height: 8,
                        background: i === currentGroupIdx ? 'var(--accent)' : 'var(--border-mid)',
                      }}
                      aria-label={`Go to group ${i + 1}`}
                      title={domainOf(g.bookmarks[0]?.url ?? '')}
                    />
                  ))}
                </div>
              )}
            </div>

            {/* Footer */}
            <div
              className="flex-none px-6 py-4 flex items-center gap-3"
              style={{ borderTop: '1px solid var(--border-dim)' }}
            >
              <div className="flex-1 text-xs" style={{ color: 'var(--text-muted)' }}>
                {resolvedCount} of {groups.length} group{groups.length === 1 ? '' : 's'} reviewed
              </div>
              <button
                type="button"
                onClick={onClose}
                className="px-4 py-2 rounded-lg text-sm font-medium transition-colors duration-150 cursor-pointer"
                style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
                onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
                onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleMergeAll}
                className="px-5 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 cursor-pointer"
                style={{ background: 'var(--accent)', color: '#0c0b0a' }}
              >
                Remove {groups.reduce((n, g) => n + g.bookmarks.length - 1, 0)} duplicate{groups.reduce((n, g) => n + g.bookmarks.length - 1, 0) === 1 ? '' : 's'}
              </button>
            </div>
          </>
        )}

        {/* merging */}
        {step === 'merging' && (
          <div className="p-10 flex flex-col items-center gap-5">
            <span
              className="inline-block w-8 h-8 rounded-full border-2 animate-spin flex-none"
              style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
            />
            <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
              Removing duplicates…
            </p>
          </div>
        )}

        {/* done */}
        {step === 'done' && (
          <div className="p-8 flex flex-col items-center gap-4">
            <div
              className="w-14 h-14 rounded-full flex items-center justify-center"
              style={{ background: 'rgba(200,160,90,0.12)' }}
              aria-hidden="true"
            >
              <IconCheck size={26} />
            </div>
            <div className="text-center">
              <p className="text-sm font-semibold" style={{ color: 'var(--text-primary)' }}>
                {mergedCount === 0
                  ? 'Nothing removed.'
                  : `Removed ${mergedCount} duplicate${mergedCount === 1 ? '' : 's'}`}
              </p>
              <p className="text-xs mt-1" style={{ color: 'var(--text-muted)' }}>
                {mergedCount > 0 ? 'Tags were merged into the kept bookmark. Duplicates are in the bin.' : ''}
              </p>
            </div>
            <button
              onClick={onClose}
              className="mt-2 px-6 py-2 rounded-lg text-sm font-semibold cursor-pointer transition-opacity duration-150 hover:opacity-90"
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
