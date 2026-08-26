import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { SearchBox } from '../components/SearchBox'
import {
  IconFilter,
  IconLayoutGrid,
  IconLayoutList,
  IconMoon,
  IconRefresh,
  IconSettings,
  IconSun,
} from '../components/icons'
import type { ViewMode } from '../types'

// Mirrors `pgsync::NeonStatus` — only the fields the header needs.
interface NeonStatus {
  enabled: boolean
  last_sync: number | null
}

function formatLastSync(secs: number | null): string {
  if (!secs) return 'never'
  return new Date(secs * 1000).toLocaleString()
}

interface MobileHeaderProps {
  /** Receives the debounced search query from the reused `SearchBox`. */
  onSearch: (value: string) => void
  viewMode: ViewMode
  onToggleView: () => void
  theme: 'dark' | 'light'
  onToggleTheme: () => void
  onOpenSettings: () => void
  /** The filter button renders only when provided — opens the FilterDrawer. */
  onOpenFilter?: () => void
  /** True while a sync cycle runs (from the `backup-syncing` event, manual or automatic). */
  syncing: boolean
}

/**
 * Top chrome of the mobile shell: title row with refresh / theme / view /
 * settings buttons, then a full-width search row. The refresh button only
 * appears once the device is paired (Neon sync enabled) and triggers the
 * pull-only `neon_sync_now`; its spinner and disabled state are driven by the
 * event-sourced `syncing` prop so automatic syncs show the same feedback.
 */
export function MobileHeader({
  onSearch,
  viewMode,
  onToggleView,
  theme,
  onToggleTheme,
  onOpenSettings,
  onOpenFilter,
  syncing,
}: MobileHeaderProps) {
  const [backup, setBackup] = useState<NeonStatus | null>(null)

  // Load sync status on mount and again after each sync cycle ends, keeping
  // the paired flag and the "last sync" line current.
  useEffect(() => {
    if (syncing) return
    let active = true
    invoke<NeonStatus | null>('neon_status')
      .then((s) => {
        if (active) setBackup(s)
      })
      .catch(() => {}) // treat as not paired — refresh stays hidden
    return () => {
      active = false
    }
  }, [syncing])

  const paired = backup?.enabled === true

  return (
    <header className="mobile-chrome">
      <div className="flex items-center gap-1 pl-4 pr-2" style={{ height: 56 }}>
        <h1 className="text-base font-semibold" style={{ fontFamily: 'var(--font-display)' }}>
          Ferrico
        </h1>
        {syncing && (
          <span
            role="status"
            className="flex items-center gap-1.5 ml-2 text-xs"
            style={{ color: 'var(--text-3)' }}
          >
            <span
              className="inline-block w-3 h-3 rounded-full border-2 animate-spin flex-none"
              style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
              aria-hidden="true"
            />
            Syncing…
          </span>
        )}
        <div className="flex-1" />
        {paired && (
          <button
            className="mobile-icon-btn"
            onClick={() => {
              // Failures surface through the `backup-error` event, which the
              // shell already renders — nothing to do with the rejection here.
              invoke('neon_sync_now').catch(() => {})
            }}
            disabled={syncing}
            aria-label="Refresh bookmarks"
          >
            <IconRefresh size={18} />
          </button>
        )}
        <button
          className="mobile-icon-btn"
          onClick={onToggleTheme}
          aria-label={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
        >
          {theme === 'dark' ? <IconSun size={16} /> : <IconMoon size={16} />}
        </button>
        <button
          className="mobile-icon-btn"
          onClick={onToggleView}
          aria-label={viewMode === 'list' ? 'Switch to grid view' : 'Switch to list view'}
        >
          {viewMode === 'list' ? <IconLayoutGrid size={16} /> : <IconLayoutList size={16} />}
        </button>
        <button className="mobile-icon-btn" onClick={onOpenSettings} aria-label="Settings">
          <IconSettings size={16} />
        </button>
      </div>

      <div className="flex items-center gap-2 px-4 pb-2.5">
        <SearchBox mobile onSearch={onSearch} />
        {onOpenFilter && (
          <button className="mobile-icon-btn" onClick={onOpenFilter} aria-label="Filter by folder or tag">
            <IconFilter size={18} />
          </button>
        )}
      </div>

      {paired && (
        <p className="px-4 pb-2 text-xs" style={{ color: 'var(--text-3)' }}>
          Last sync: {formatLastSync(backup!.last_sync)}
        </p>
      )}
    </header>
  )
}
