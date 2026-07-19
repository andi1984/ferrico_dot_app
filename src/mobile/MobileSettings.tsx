import { useCallback, useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { version as APP_VERSION } from '../../package.json'
import { SettingsLayout } from '../components/SettingsLayout'
import { FieldLabel } from '../components/ModalShell'
import { IconRestore, IconSun, IconMoon } from '../components/icons'
import { extractErrorMessage } from '../utils'

// Mirrors `gdrive::BackupStatus` (serde snake_case) — same shape BackupSettingsPage uses.
interface BackupStatus {
  has_credentials: boolean
  connected: boolean
  account_email: string | null
  folder_id: string | null
  folder_name: string | null
  last_sync: string | null
  interval_min: number
  enabled: boolean
}

type Theme = 'dark' | 'light'

function formatLastSync(iso: string | null): string {
  if (!iso) return 'never'
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  return d.toLocaleString()
}

export function MobileSettings({ onClose, theme, onToggleTheme }: {
  onClose: () => void
  theme: Theme
  onToggleTheme: () => void
}) {
  const [status, setStatus] = useState<BackupStatus | null>(null)
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
    invoke<BackupStatus>('backup_status')
      .then(setStatus)
      .catch((e) => setError(extractErrorMessage(e)))
  }, [])

  function importPairing() {
    if (!pairingInput.trim()) return
    run(
      'import',
      () => invoke<BackupStatus>('backup_import_pairing', { payload: pairingInput.trim() }),
      (s) => {
        setStatus(s)
        setPairingInput('')
      },
    )
  }

  function syncNow() {
    run('sync', () => invoke<BackupStatus>('backup_sync_now'), setStatus)
  }

  function unpair() {
    run('unpair', () => invoke<BackupStatus>('backup_disconnect'), setStatus)
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
      ) : !status.connected ? (
        // ── Pairing import ──
        <div className="flex flex-col gap-3">
          <FieldLabel>Pair with desktop</FieldLabel>
          <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
            On your desktop, open Settings → Cloud Backup → Pair a mobile device, then paste
            the code here. This device only ever pulls — it never writes back to Drive.
          </p>
          <textarea
            value={pairingInput}
            onChange={(e) => setPairingInput(e.target.value)}
            placeholder="ferrico-pair:v1:…"
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
            <FieldLabel>Account</FieldLabel>
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs truncate" style={{ color: 'var(--text-1)' }}>
                {status.account_email ?? 'Connected'}
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

          {status.folder_name && (
            <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }}>
              <FieldLabel>Folder</FieldLabel>
              <span className="text-xs" style={{ color: 'var(--text-1)' }}>{status.folder_name}</span>
            </div>
          )}

          <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }} className="flex items-center justify-between gap-2">
            <div className="flex flex-col gap-0.5">
              <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
                Last synced: {formatLastSync(status.last_sync)}
              </span>
              <span className="text-xs" style={{ color: 'var(--text-3)' }}>
                This device is download-only — it never pushes changes back to Drive.
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
