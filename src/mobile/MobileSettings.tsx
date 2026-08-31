import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { version as APP_VERSION } from '../../package.json'
import { SettingsLayout } from '../components/SettingsLayout'
import { FieldLabel } from '../components/ModalShell'
import { IconRestore, IconSun, IconMoon } from '../components/icons'
import { extractErrorMessage } from '../utils'

// Mirrors `pgsync::NeonStatus` (serde snake_case) — same shape BackupSettingsPage uses.
interface NeonStatus {
  configured: boolean
  enabled: boolean
  host: string | null
  dbname: string
  user: string | null
  interval_min: number
  last_seq: number
  last_sync: number | null
}

type Theme = 'dark' | 'light'

function formatLastSync(secs: number | null): string {
  if (!secs) return 'never'
  return new Date(secs * 1000).toLocaleString()
}

export function MobileSettings({ onClose, theme, onToggleTheme }: {
  onClose: () => void
  theme: Theme
  onToggleTheme: () => void
}) {
  const [status, setStatus] = useState<NeonStatus | null>(null)
  const [pairingInput, setPairingInput] = useState('')
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const run = useCallback(
    async <T,>(action: string, fn: () => Promise<T>, after?: (r: T) => void) => {
      setBusy(action)
      setError(null)
      try {
        const r = await fn()
        after?.(r)
      } catch (e) {
        setError(extractErrorMessage(e))
      } finally {
        setBusy(null)
      }
    },
    [],
  )

  useEffect(() => {
    invoke<NeonStatus>('neon_status')
      .then(setStatus)
      .catch((e) => setError(extractErrorMessage(e)))
  }, [])

  function importPairing() {
    if (!pairingInput.trim()) return
    run(
      'import',
      () => invoke<NeonStatus>('backup_import_pairing', { payload: pairingInput.trim() }),
      (s) => {
        setStatus(s)
        setPairingInput('')
      },
    )
  }

  function syncNow() {
    run('sync', () => invoke<NeonStatus>('neon_sync_now'), setStatus)
  }

  function unpair() {
    run('unpair', () => invoke<NeonStatus>('neon_disconnect'), setStatus)
  }

  function setInterval(intervalMin: number) {
    run('interval', () => invoke<NeonStatus>('neon_set_interval', { intervalMin }), setStatus)
  }

  const spinner = (
    <span
      className="inline-block w-3 h-3 rounded-full border-2 animate-spin flex-none"
      style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
    />
  )

  return (
    <SettingsLayout breadcrumb={[{ label: 'Settings' }]} onBack={onClose}>
      {error && (
        <div
          role="alert"
          className="rounded-lg px-4 py-3 text-xs"
          style={{ background: 'rgba(224,82,82,0.08)', color: 'var(--red)', border: '1px solid rgba(224,82,82,0.2)' }}
        >
          {error}
        </div>
      )}

      {!status ? (
        <div className="flex items-center gap-2 text-xs" style={{ color: 'var(--text-2)' }}>
          {spinner} Loading…
        </div>
      ) : !status.configured || !status.enabled ? (
        // ── Pairing import ──
        <div className="flex flex-col gap-3">
          <FieldLabel>Pair with desktop</FieldLabel>
          <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
            On your desktop, open Settings → Sync &amp; Backup → Pair a mobile device, then
            paste the code here. Changes sync both ways — edits on this device push to
            your database.
          </p>
          <textarea
            value={pairingInput}
            onChange={(e) => setPairingInput(e.target.value)}
            placeholder="ferrico-pair:v2:…"
            rows={4}
            aria-label="Pairing code"
            className="w-full px-3 py-2 rounded-lg text-xs font-mono resize-none"
            style={{ background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)', wordBreak: 'break-all' }}
          />
          <button
            onClick={importPairing}
            disabled={busy !== null || !pairingInput.trim()}
            className="btn-accent rounded-lg px-4 cursor-pointer flex items-center justify-center gap-2 self-start"
            style={{ height: 36, fontSize: 13, fontWeight: 500, opacity: busy || !pairingInput.trim() ? 0.6 : 1 }}
          >
            {busy === 'import' && spinner}
            Pair this device
          </button>
        </div>
      ) : (
        // ── Paired dashboard ──
        <div className="flex flex-col gap-5">
          <div>
            <FieldLabel>Sync database</FieldLabel>
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs truncate font-mono" style={{ color: 'var(--text-1)' }}>
                {status.user}@{status.host}
              </span>
              <button
                onClick={unpair}
                disabled={busy !== null}
                className="rounded-lg px-3 cursor-pointer flex-none"
                style={{ height: 28, fontSize: 11.5, fontWeight: 500, color: 'var(--red)', background: 'transparent', border: '1px solid var(--border-soft)' }}
              >
                {busy === 'unpair' ? 'Unpairing…' : 'Unpair'}
              </button>
            </div>
          </div>

          <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }} className="flex items-center justify-between gap-2">
            <div className="flex flex-col gap-0.5">
              <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
                Last synced: {formatLastSync(status.last_sync)}
              </span>
              <span className="text-xs" style={{ color: 'var(--text-3)' }}>
                Syncs both ways — edits here push to your database.
              </span>
            </div>
            <button
              onClick={syncNow}
              disabled={busy !== null}
              className="rounded-lg px-4 cursor-pointer flex items-center gap-2 flex-none"
              style={{ height: 32, fontSize: 12, fontWeight: 500, border: '1px solid var(--accent)', color: 'var(--accent)', background: 'transparent', opacity: busy ? 0.6 : 1 }}
            >
              {busy === 'sync' ? spinner : <IconRestore size={13} />}
              {busy === 'sync' ? 'Syncing…' : 'Sync now'}
            </button>
          </div>

          <div className="flex items-center justify-between gap-2">
            <label htmlFor="sync-interval" className="text-xs" style={{ color: 'var(--text-2)' }}>
              Also pull remote changes every
            </label>
            <select
              id="sync-interval"
              value={status.interval_min}
              disabled={busy !== null}
              onChange={(e) => setInterval(Number(e.target.value))}
              className="px-2 py-1 rounded-md text-xs"
              style={{ background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
            >
              <option value={0}>only on open / resume</option>
              <option value={15}>15 min</option>
              <option value={30}>30 min</option>
              <option value={60}>1 hour</option>
              <option value={360}>6 hours</option>
            </select>
          </div>
        </div>
      )}

      <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }} className="flex items-center justify-between gap-2">
        <FieldLabel>Theme</FieldLabel>
        <button
          onClick={onToggleTheme}
          className="mobile-icon-btn"
          aria-label={theme === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
        >
          {theme === 'dark' ? <IconSun size={16} /> : <IconMoon size={16} />}
        </button>
      </div>

      <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }}>
        <span className="text-xs" style={{ color: 'var(--text-muted)' }}>Ferrico v{APP_VERSION}</span>
      </div>
    </SettingsLayout>
  )
}
