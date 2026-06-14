import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ModalShell, FieldLabel } from './ModalShell'
import { IconFolder, IconRestore, IconCheck, IconPlus } from './icons'
import { extractErrorMessage } from '../utils'

// Mirrors `gdrive::BackupStatus` (serde snake_case).
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

interface DriveFolder {
  id: string
  name: string
}

const CONSOLE_URL = 'https://console.cloud.google.com/apis/credentials'

function formatLastSync(iso: string | null): string {
  if (!iso) return 'never'
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  return d.toLocaleString()
}

export function BackupSettingsModal({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [status, setStatus] = useState<BackupStatus | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  // Credentials form
  const [editingCreds, setEditingCreds] = useState(false)
  const [clientId, setClientId] = useState('')
  const [clientSecret, setClientSecret] = useState('')

  // Folder picker
  const [pickingFolder, setPickingFolder] = useState(false)
  const [folders, setFolders] = useState<DriveFolder[] | null>(null)
  const [newFolderName, setNewFolderName] = useState('Ferrico Backups')

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

  const loadFolders = useCallback(async () => {
    setBusy('folders')
    setError(null)
    try {
      setFolders(await invoke<DriveFolder[]>('backup_list_folders'))
    } catch (e) {
      setError(extractErrorMessage(e))
      setFolders([]) // stop the spinner; reveal the "create folder" path + error
    } finally {
      setBusy(null)
    }
  }, [])

  // Auto-load the folder list whenever the picker view is showing with no data
  // yet. `folders === null` is the only trigger, so a resolved fetch (array or
  // []) never re-fires — no infinite loop on an empty list or an API error.
  useEffect(() => {
    if (status && status.connected && (pickingFolder || !status.folder_id) && folders === null) {
      loadFolders()
    }
  }, [status, pickingFolder, folders, loadFolders])

  function saveCredentials() {
    run(
      'creds',
      () => invoke<BackupStatus>('backup_set_credentials', { clientId, clientSecret }),
      (s) => {
        setStatus(s)
        setEditingCreds(false)
        setClientSecret('')
      },
    )
  }

  function connect() {
    run('connect', () => invoke<BackupStatus>('backup_connect'), setStatus)
  }

  function disconnect() {
    run('disconnect', () => invoke<BackupStatus>('backup_disconnect'), setStatus)
  }

  function openFolderPicker() {
    setPickingFolder(true)
    setFolders(null)
    run('folders', () => invoke<DriveFolder[]>('backup_list_folders'), setFolders)
  }

  function selectFolder(f: DriveFolder) {
    run(
      'select',
      () => invoke<BackupStatus>('backup_select_folder', { folderId: f.id, folderName: f.name }),
      (s) => {
        setStatus(s)
        setPickingFolder(false)
      },
    )
  }

  function createFolder() {
    run(
      'create',
      () => invoke<DriveFolder>('backup_create_folder', { name: newFolderName }),
      () => {
        // The backend auto-selects the new folder; refresh status to reflect it.
        invoke<BackupStatus>('backup_status').then((s) => {
          setStatus(s)
          setPickingFolder(false)
        })
      },
    )
  }

  function toggleEnabled(enabled: boolean) {
    run('enabled', () => invoke<BackupStatus>('backup_set_enabled', { enabled }), setStatus)
  }

  function setInterval(intervalMin: number) {
    run('interval', () => invoke<BackupStatus>('backup_set_interval', { intervalMin }), setStatus)
  }

  function syncNow() {
    run('sync', () => invoke<BackupStatus>('backup_sync_now'), (s) => {
      setStatus(s)
      onDone()
    })
  }

  const spinner = (
    <span
      className="inline-block w-3 h-3 rounded-full border-2 animate-spin flex-none"
      style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
    />
  )

  return (
    <ModalShell title="Cloud Backup" onClose={onClose}>
      <div className="p-6 flex flex-col gap-5">
        {error && (
          <div
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
        ) : !status.has_credentials || editingCreds ? (
          // ── Credentials form ──
          <div className="flex flex-col gap-3">
            <FieldLabel>Google OAuth Client</FieldLabel>
            <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
              Backups go to your own Google Drive, so Ferrico needs a Google OAuth client
              (one-time setup). In Google Cloud Console create an OAuth client of type
              <strong> Desktop app</strong>, then paste its ID and secret below.
            </p>
            <button
              onClick={() => invoke('open_url', { url: CONSOLE_URL }).catch(() => {})}
              className="self-start text-xs underline"
              style={{ color: 'var(--accent)' }}
            >
              Open Google Cloud credentials →
            </button>

            <div>
              <FieldLabel htmlFor="g-client-id">Client ID</FieldLabel>
              <input
                id="g-client-id"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="xxxxx.apps.googleusercontent.com"
                className="w-full px-3 py-2 rounded-lg text-xs font-mono"
                style={{ background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
              />
            </div>
            <div>
              <FieldLabel htmlFor="g-client-secret">Client Secret</FieldLabel>
              <input
                id="g-client-secret"
                type="password"
                value={clientSecret}
                onChange={(e) => setClientSecret(e.target.value)}
                placeholder="GOCSPX-…"
                className="w-full px-3 py-2 rounded-lg text-xs font-mono"
                style={{ background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
              />
            </div>
            <div className="flex gap-2 pt-1">
              {editingCreds && (
                <button
                  onClick={() => setEditingCreds(false)}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 32, fontSize: 12, fontWeight: 500, background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
                >
                  Cancel
                </button>
              )}
              <button
                onClick={saveCredentials}
                disabled={busy !== null || !clientId.trim() || !clientSecret.trim()}
                className="btn-accent rounded-lg px-4 cursor-pointer flex items-center gap-2"
                style={{ height: 32, fontSize: 12, fontWeight: 500, opacity: busy || !clientId.trim() || !clientSecret.trim() ? 0.6 : 1 }}
              >
                {busy === 'creds' && spinner}
                Save credentials
              </button>
            </div>
          </div>
        ) : !status.connected ? (
          // ── Connect ──
          <div className="flex flex-col gap-3">
            <FieldLabel>Connect</FieldLabel>
            <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
              Authorize Ferrico to access its own backup file in your Google Drive. A browser
              window will open for Google sign-in.
            </p>
            <div className="flex gap-2">
              <button
                onClick={connect}
                disabled={busy !== null}
                className="btn-accent rounded-lg px-4 cursor-pointer flex items-center gap-2"
                style={{ height: 32, fontSize: 12, fontWeight: 500, opacity: busy ? 0.6 : 1 }}
              >
                {busy === 'connect' ? spinner : <IconCheck size={13} />}
                {busy === 'connect' ? 'Waiting for Google…' : 'Connect Google Drive'}
              </button>
              <button
                onClick={() => setEditingCreds(true)}
                className="rounded-lg px-3 cursor-pointer"
                style={{ height: 32, fontSize: 12, fontWeight: 500, background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
              >
                Edit credentials
              </button>
            </div>
          </div>
        ) : pickingFolder || !status.folder_id ? (
          // ── Folder picker ──
          <div className="flex flex-col gap-3">
            <FieldLabel>Backup folder</FieldLabel>
            <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
              Choose a Drive folder for the backup file. Folders created here also show up in
              your Google Drive; on another machine, sign in with the same account and pick the
              same folder to stay in sync.
            </p>

            <div className="flex flex-col gap-2">
              {folders === null ? (
                <div className="flex items-center gap-2 text-xs" style={{ color: 'var(--text-2)' }}>{spinner} Loading folders…</div>
              ) : folders.length === 0 ? (
                <p className="text-xs" style={{ color: 'var(--text-muted)' }}>No folders yet — create one below.</p>
              ) : (
                <div className="flex flex-col gap-1 max-h-44 overflow-auto">
                  {folders.map((f) => (
                    <button
                      key={f.id}
                      onClick={() => selectFolder(f)}
                      disabled={busy !== null}
                      className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs text-left cursor-pointer transition-colors"
                      style={{ background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
                      onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--btn-hover-bg)')}
                      onMouseLeave={(e) => (e.currentTarget.style.background = 'var(--input-bg)')}
                    >
                      <IconFolder size={13} />
                      {f.name}
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div className="flex gap-2 pt-1">
              <input
                value={newFolderName}
                onChange={(e) => setNewFolderName(e.target.value)}
                placeholder="New folder name"
                className="flex-1 px-3 py-2 rounded-lg text-xs"
                style={{ background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
              />
              <button
                onClick={createFolder}
                disabled={busy !== null || !newFolderName.trim()}
                className="rounded-lg px-3 cursor-pointer flex items-center gap-1.5"
                style={{ height: 34, fontSize: 12, fontWeight: 500, border: '1px solid var(--accent)', color: 'var(--accent)', opacity: busy || !newFolderName.trim() ? 0.6 : 1 }}
              >
                {busy === 'create' ? spinner : <IconPlus size={13} />}
                Create
              </button>
            </div>

            {status.folder_id && (
              <button
                onClick={() => setPickingFolder(false)}
                className="self-start text-xs underline"
                style={{ color: 'var(--text-2)' }}
              >
                Cancel
              </button>
            )}
          </div>
        ) : (
          // ── Connected dashboard ──
          <div className="flex flex-col gap-5">
            <div>
              <FieldLabel>Account</FieldLabel>
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs truncate" style={{ color: 'var(--text-1)' }}>
                  {status.account_email ?? 'Connected'}
                </span>
                <button
                  onClick={disconnect}
                  disabled={busy !== null}
                  className="rounded-lg px-3 cursor-pointer flex-none"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, color: 'var(--red)', background: 'transparent', border: '1px solid var(--border-soft)' }}
                  onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--red)')}
                  onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-soft)')}
                >
                  {busy === 'disconnect' ? 'Disconnecting…' : 'Disconnect'}
                </button>
              </div>
            </div>

            <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }}>
              <FieldLabel>Folder</FieldLabel>
              <div className="flex items-center justify-between gap-2">
                <span className="flex items-center gap-2 text-xs truncate" style={{ color: 'var(--text-1)' }}>
                  <IconFolder size={13} />
                  {status.folder_name ?? status.folder_id}
                </span>
                <button
                  onClick={openFolderPicker}
                  className="rounded-lg px-3 cursor-pointer flex-none"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
                >
                  Change
                </button>
              </div>
            </div>

            <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }} className="flex flex-col gap-3">
              <FieldLabel>Automatic sync</FieldLabel>
              <label className="flex items-center gap-2.5 cursor-pointer text-xs" style={{ color: 'var(--text-1)' }}>
                <input
                  type="checkbox"
                  checked={status.enabled}
                  disabled={busy !== null}
                  onChange={(e) => toggleEnabled(e.target.checked)}
                  style={{ accentColor: 'var(--accent)', width: 15, height: 15 }}
                />
                Pull on launch &amp; back up before close
              </label>

              <div className="flex items-center gap-2 text-xs" style={{ color: 'var(--text-2)' }}>
                <span>Also back up every</span>
                <input
                  type="number"
                  min={0}
                  value={status.interval_min}
                  disabled={busy !== null}
                  onChange={(e) => setInterval(Math.max(0, Math.floor(Number(e.target.value) || 0)))}
                  className="px-2 py-1 rounded-md text-xs text-center"
                  style={{ width: 56, background: 'var(--input-bg)', border: '1px solid var(--border-soft)', color: 'var(--text-1)' }}
                />
                <span>min while running (0 = off)</span>
              </div>
            </div>

            <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' }} className="flex items-center justify-between gap-2">
              <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
                Last synced: {formatLastSync(status.last_sync)}
              </span>
              <button
                onClick={syncNow}
                disabled={busy !== null}
                className="rounded-lg px-4 cursor-pointer flex items-center gap-2 flex-none"
                style={{ height: 32, fontSize: 12, fontWeight: 500, border: '1px solid var(--accent)', color: 'var(--accent)', background: 'transparent', opacity: busy ? 0.6 : 1 }}
                onMouseEnter={(e) => { if (!busy) e.currentTarget.style.background = 'var(--accent-dim)' }}
                onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
              >
                {busy === 'sync' ? spinner : <IconRestore size={13} />}
                {busy === 'sync' ? 'Syncing…' : 'Sync now'}
              </button>
            </div>
          </div>
        )}
      </div>
    </ModalShell>
  )
}
