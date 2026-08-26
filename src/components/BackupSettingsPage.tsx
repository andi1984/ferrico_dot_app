import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import QRCode from 'qrcode'
import { FieldLabel } from './ModalShell'
import { SettingsLayout } from './SettingsLayout'
import { IconFolder, IconRestore, IconCheck, IconPlus } from './icons'
import { extractErrorMessage } from '../utils'

// Mirrors `pgsync::NeonStatus` (serde snake_case).
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
const NEON_URL = 'https://neon.tech'

function formatLastSyncIso(iso: string | null): string {
  if (!iso) return 'never'
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  return d.toLocaleString()
}

function formatLastSyncEpoch(secs: number | null): string {
  if (!secs) return 'never'
  return new Date(secs * 1000).toLocaleString()
}

const inputStyle = {
  background: 'var(--input-bg)',
  border: '1px solid var(--border-soft)',
  color: 'var(--text-1)',
} as const

export function BackupSettingsPage({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  const run = useCallback(
    async <T,>(action: string, fn: () => Promise<T>, after?: (r: T) => void) => {
      setBusy(action)
      setError(null)
      setNotice(null)
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

  // ── Neon sync state ──
  const [neon, setNeon] = useState<NeonStatus | null>(null)
  const [editingNeon, setEditingNeon] = useState(false)
  const [host, setHost] = useState('')
  const [dbname, setDbname] = useState('')
  const [user, setUser] = useState('')
  const [password, setPassword] = useState('')

  // ── Drive state ──
  const [status, setStatus] = useState<BackupStatus | null>(null)
  const [editingCreds, setEditingCreds] = useState(false)
  const [clientId, setClientId] = useState('')
  const [clientSecret, setClientSecret] = useState('')
  const [pickingFolder, setPickingFolder] = useState(false)
  const [folders, setFolders] = useState<DriveFolder[] | null>(null)
  const [newFolderName, setNewFolderName] = useState('Ferrico Backups')
  const [confirmingRestore, setConfirmingRestore] = useState(false)

  // ── Mobile pairing ──
  const [pairingCode, setPairingCode] = useState<string | null>(null)
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null)
  const [pairingCopied, setPairingCopied] = useState(false)

  useEffect(() => {
    invoke<NeonStatus>('neon_status')
      .then((s) => {
        setNeon(s)
        setHost(s.host ?? '')
        setDbname(s.dbname ?? '')
        setUser(s.user ?? '')
      })
      .catch((e) => setError(extractErrorMessage(e)))
    invoke<BackupStatus>('backup_status')
      .then(setStatus)
      .catch((e) => setError(extractErrorMessage(e)))
  }, [])

  // ── Neon actions ──

  function saveNeonConfig() {
    run(
      'neon-save',
      () => invoke<NeonStatus>('neon_set_config', { host, dbname, user, password }),
      (s) => {
        setNeon(s)
        setPassword('')
        setEditingNeon(false)
      },
    )
  }

  function testNeon() {
    run('neon-test', () => invoke('neon_test_connection'), () => setNotice('Connection OK — schema is ready.'))
  }

  function toggleNeonEnabled(enabled: boolean) {
    run('neon-enabled', () => invoke<NeonStatus>('neon_set_enabled', { enabled }), setNeon)
  }

  function setNeonInterval(intervalMin: number) {
    run('neon-interval', () => invoke<NeonStatus>('neon_set_interval', { intervalMin }), setNeon)
  }

  function neonSyncNow() {
    run('neon-sync', () => invoke<NeonStatus>('neon_sync_now'), (s) => {
      setNeon(s)
      onDone()
    })
  }

  function neonDisconnect() {
    run('neon-disconnect', () => invoke<NeonStatus>('neon_disconnect'), (s) => {
      setNeon(s)
      setEditingNeon(false)
    })
  }

  // ── Drive actions ──

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

  function exportNow() {
    run('export', () => invoke<BackupStatus>('backup_export_now'), (s) => {
      setStatus(s)
      setNotice('Backup uploaded to Google Drive.')
    })
  }

  function restoreFromDrive() {
    setConfirmingRestore(false)
    run('restore', () => invoke<BackupStatus>('backup_restore_from_drive'), (s) => {
      setStatus(s)
      setNotice('Restore complete. A safety export of the previous data was saved beside the database.')
      onDone()
    })
  }

  // ── Pairing ──

  function showPairing() {
    run(
      'pairing',
      async () => {
        const code = await invoke<string>('backup_export_pairing')
        // QR failure (unexpected) still leaves the copy-paste string usable.
        const qr = await QRCode.toDataURL(code, { errorCorrectionLevel: 'M' }).catch(() => null)
        return { code, qr }
      },
      ({ code, qr }) => {
        setPairingCode(code)
        setQrDataUrl(qr)
      },
    )
  }

  function copyPairing() {
    if (!pairingCode) return
    navigator.clipboard.writeText(pairingCode).then(() => {
      setPairingCopied(true)
      setTimeout(() => setPairingCopied(false), 2000)
    })
  }

  const spinner = (
    <span
      className="inline-block w-3 h-3 rounded-full border-2 animate-spin flex-none"
      style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
    />
  )

  const sectionTop = { borderTop: '1px solid var(--border-dim)', paddingTop: '1.25rem' } as const

  return (
    <SettingsLayout breadcrumb={[{ label: 'Settings', onClick: onClose }, { label: 'Sync & Backup' }]} onBack={onClose}>
      {error && (
        <div
          className="rounded-lg px-4 py-3 text-xs"
          style={{ background: 'rgba(224,82,82,0.08)', color: 'var(--red)', border: '1px solid rgba(224,82,82,0.2)' }}
        >
          {error}
        </div>
      )}
      {notice && (
        <div
          className="rounded-lg px-4 py-3 text-xs"
          style={{ background: 'var(--accent-dim)', color: 'var(--accent)', border: '1px solid var(--border-soft)' }}
        >
          {notice}
        </div>
      )}

      {/* ── Neon sync (primary) ── */}
      {!neon ? (
        <div className="flex items-center gap-2 text-xs" style={{ color: 'var(--text-2)' }}>
          {spinner} Loading…
        </div>
      ) : !neon.configured || editingNeon ? (
        <div className="flex flex-col gap-3">
          <FieldLabel>Neon Sync</FieldLabel>
          <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
            Bookmarks sync through your own Postgres database — create a free project on Neon,
            then paste its host and role credentials below. Ferrico manages the tables itself.
          </p>
          <button
            onClick={() => invoke('open_url', { url: NEON_URL }).catch(() => {})}
            className="self-start text-xs underline"
            style={{ color: 'var(--accent)' }}
          >
            Open neon.tech →
          </button>

          <div>
            <FieldLabel htmlFor="neon-host">Host</FieldLabel>
            <input
              id="neon-host"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              placeholder="ep-xxxx-xxxx.eu-central-1.aws.neon.tech"
              className="w-full px-3 py-2 rounded-lg text-xs font-mono"
              style={inputStyle}
            />
          </div>
          <div className="flex gap-2">
            <div className="flex-1">
              <FieldLabel htmlFor="neon-user">User</FieldLabel>
              <input
                id="neon-user"
                value={user}
                onChange={(e) => setUser(e.target.value)}
                placeholder="neondb_owner"
                className="w-full px-3 py-2 rounded-lg text-xs font-mono"
                style={inputStyle}
              />
            </div>
            <div className="flex-1">
              <FieldLabel htmlFor="neon-db">Database</FieldLabel>
              <input
                id="neon-db"
                value={dbname}
                onChange={(e) => setDbname(e.target.value)}
                placeholder="neondb"
                className="w-full px-3 py-2 rounded-lg text-xs font-mono"
                style={inputStyle}
              />
            </div>
          </div>
          <div>
            <FieldLabel htmlFor="neon-password">Password</FieldLabel>
            <input
              id="neon-password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={neon.configured ? '(unchanged)' : 'npg_…'}
              className="w-full px-3 py-2 rounded-lg text-xs font-mono"
              style={inputStyle}
            />
            <p className="text-xs mt-1" style={{ color: 'var(--text-3)' }}>
              Stored in your OS keychain where available.
            </p>
          </div>
          <div className="flex gap-2 pt-1">
            {editingNeon && (
              <button
                onClick={() => setEditingNeon(false)}
                className="rounded-lg px-3 cursor-pointer"
                style={{ height: 32, fontSize: 12, fontWeight: 500, ...inputStyle }}
              >
                Cancel
              </button>
            )}
            <button
              onClick={saveNeonConfig}
              disabled={busy !== null || !host.trim() || !user.trim()}
              className="btn-accent rounded-lg px-4 cursor-pointer flex items-center gap-2"
              style={{ height: 32, fontSize: 12, fontWeight: 500, opacity: busy || !host.trim() || !user.trim() ? 0.6 : 1 }}
            >
              {busy === 'neon-save' && spinner}
              Save connection
            </button>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-5">
          <div>
            <FieldLabel>Neon Sync</FieldLabel>
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs truncate font-mono" style={{ color: 'var(--text-1)' }}>
                {neon.user}@{neon.host}/{neon.dbname}
              </span>
              <div className="flex gap-2 flex-none">
                <button
                  onClick={() => setEditingNeon(true)}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, ...inputStyle }}
                >
                  Edit
                </button>
                <button
                  onClick={neonDisconnect}
                  disabled={busy !== null}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, color: 'var(--red)', background: 'transparent', border: '1px solid var(--border-soft)' }}
                  onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--red)')}
                  onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-soft)')}
                >
                  {busy === 'neon-disconnect' ? 'Disconnecting…' : 'Disconnect'}
                </button>
              </div>
            </div>
          </div>

          <div style={sectionTop} className="flex flex-col gap-3">
            <label className="flex items-center gap-2.5 cursor-pointer text-xs" style={{ color: 'var(--text-1)' }}>
              <input
                type="checkbox"
                checked={neon.enabled}
                disabled={busy !== null}
                onChange={(e) => toggleNeonEnabled(e.target.checked)}
                style={{ accentColor: 'var(--accent)', width: 15, height: 15 }}
              />
              Sync automatically (pull on launch, push changes within seconds)
            </label>

            <div className="flex items-center gap-2 text-xs" style={{ color: 'var(--text-2)' }}>
              <span>Also pull remote changes every</span>
              <input
                type="number"
                min={0}
                value={neon.interval_min}
                disabled={busy !== null}
                onChange={(e) => setNeonInterval(Math.max(0, Math.floor(Number(e.target.value) || 0)))}
                className="px-2 py-1 rounded-md text-xs text-center"
                style={{ width: 56, ...inputStyle }}
              />
              <span>min while running (0 = off)</span>
            </div>
          </div>

          <div style={sectionTop} className="flex items-center justify-between gap-2">
            <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
              Last synced: {formatLastSyncEpoch(neon.last_sync)}
            </span>
            <div className="flex gap-2 flex-none">
              <button
                onClick={testNeon}
                disabled={busy !== null}
                className="rounded-lg px-3 cursor-pointer flex items-center gap-2"
                style={{ height: 32, fontSize: 12, fontWeight: 500, ...inputStyle, opacity: busy ? 0.6 : 1 }}
              >
                {busy === 'neon-test' && spinner}
                Test connection
              </button>
              <button
                onClick={neonSyncNow}
                disabled={busy !== null}
                className="rounded-lg px-4 cursor-pointer flex items-center gap-2"
                style={{ height: 32, fontSize: 12, fontWeight: 500, border: '1px solid var(--accent)', color: 'var(--accent)', background: 'transparent', opacity: busy ? 0.6 : 1 }}
                onMouseEnter={(e) => { if (!busy) e.currentTarget.style.background = 'var(--accent-dim)' }}
                onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
              >
                {busy === 'neon-sync' ? spinner : <IconRestore size={13} />}
                {busy === 'neon-sync' ? 'Syncing…' : 'Sync now'}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* ── Mobile pairing (needs at least one pairable backend: Neon, or
             Drive with a folder — the Drive block can't export without one) ── */}
      {(neon?.configured || (status?.connected && status?.folder_id)) && (
        <div style={sectionTop} className="flex flex-col gap-3">
          <FieldLabel>Pair a mobile device</FieldLabel>
          {!pairingCode ? (
            <>
              <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
                Give the Ferrico mobile app read-only access to your synced bookmarks: scan a
                QR code or paste a pairing code — no sign-in or password typing on the phone.
              </p>
              <button
                onClick={showPairing}
                disabled={busy !== null}
                className="self-start rounded-lg px-4 cursor-pointer flex items-center gap-2"
                style={{ height: 32, fontSize: 12, fontWeight: 500, border: '1px solid var(--accent)', color: 'var(--accent)', background: 'transparent', opacity: busy ? 0.6 : 1 }}
                onMouseEnter={(e) => { if (!busy) e.currentTarget.style.background = 'var(--accent-dim)' }}
                onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
              >
                {busy === 'pairing' && spinner}
                Show pairing code
              </button>
            </>
          ) : (
            <>
              {qrDataUrl && (
                <img
                  src={qrDataUrl}
                  alt="Pairing QR code"
                  width={192}
                  height={192}
                  className="rounded-lg self-start"
                  style={{ background: '#fff', padding: 8, border: '1px solid var(--border-soft)' }}
                />
              )}
              <textarea
                readOnly
                value={pairingCode}
                rows={3}
                onFocus={(e) => e.currentTarget.select()}
                aria-label="Pairing code"
                className="w-full px-3 py-2 rounded-lg text-xs font-mono resize-none"
                style={{ ...inputStyle, wordBreak: 'break-all' }}
              />
              <div className="flex gap-2">
                <button
                  onClick={copyPairing}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, border: '1px solid var(--accent)', color: 'var(--accent)', background: 'transparent' }}
                >
                  {pairingCopied ? 'Copied!' : 'Copy'}
                </button>
                <button
                  onClick={() => { setPairingCode(null); setQrDataUrl(null) }}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, ...inputStyle }}
                >
                  Hide
                </button>
              </div>
              <p className="text-xs" style={{ color: 'var(--red)', lineHeight: 1.5 }}>
                This code contains your database credentials. Only transfer it over a channel
                you trust, and keep it out of chats, screenshots, and backups.
              </p>
            </>
          )}
        </div>
      )}

      {/* ── Google Drive manual backup ── */}
      <div style={sectionTop} className="flex flex-col gap-3">
        <FieldLabel>Google Drive backup (manual)</FieldLabel>
        {!status ? (
          <div className="flex items-center gap-2 text-xs" style={{ color: 'var(--text-2)' }}>
            {spinner} Loading…
          </div>
        ) : !status.has_credentials || editingCreds ? (
          // ── Credentials form ──
          <div className="flex flex-col gap-3">
            <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
              Optional: keep a backup file in your own Google Drive, exported and restored
              manually. Requires a Google OAuth client of type <strong>Desktop app</strong>
              (one-time setup) — paste its ID and secret below.
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
                style={inputStyle}
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
                style={inputStyle}
              />
            </div>
            <div className="flex gap-2 pt-1">
              {editingCreds && (
                <button
                  onClick={() => setEditingCreds(false)}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 32, fontSize: 12, fontWeight: 500, ...inputStyle }}
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
                style={{ height: 32, fontSize: 12, fontWeight: 500, ...inputStyle }}
              >
                Edit credentials
              </button>
            </div>
          </div>
        ) : pickingFolder || !status.folder_id ? (
          // ── Folder picker ──
          <div className="flex flex-col gap-3">
            <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
              Choose a Drive folder for the backup file. Folders created here also show up in
              your Google Drive.
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
                      style={inputStyle}
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
                style={inputStyle}
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
          // ── Connected: manual export / restore ──
          <div className="flex flex-col gap-4">
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs truncate" style={{ color: 'var(--text-1)' }}>
                {status.account_email ?? 'Connected'}
                <span style={{ color: 'var(--text-3)' }}> · </span>
                <span className="inline-flex items-center gap-1" style={{ color: 'var(--text-2)' }}>
                  <IconFolder size={12} />
                  {status.folder_name ?? status.folder_id}
                </span>
              </span>
              <div className="flex gap-2 flex-none">
                <button
                  onClick={openFolderPicker}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, ...inputStyle }}
                >
                  Change folder
                </button>
                <button
                  onClick={disconnect}
                  disabled={busy !== null}
                  className="rounded-lg px-3 cursor-pointer"
                  style={{ height: 28, fontSize: 11.5, fontWeight: 500, color: 'var(--red)', background: 'transparent', border: '1px solid var(--border-soft)' }}
                  onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--red)')}
                  onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-soft)')}
                >
                  {busy === 'disconnect' ? 'Disconnecting…' : 'Disconnect'}
                </button>
              </div>
            </div>

            <p className="text-xs" style={{ color: 'var(--text-2)', lineHeight: 1.5 }}>
              Drive is a manual fallback — nothing happens automatically. Export uploads a
              snapshot of your current bookmarks; Restore replaces this device's data with
              the backup file.
            </p>

            {!confirmingRestore ? (
              <div className="flex items-center justify-between gap-2">
                <span className="text-xs" style={{ color: 'var(--text-muted)' }}>
                  Last export/restore: {formatLastSyncIso(status.last_sync)}
                </span>
                <div className="flex gap-2 flex-none">
                  <button
                    onClick={exportNow}
                    disabled={busy !== null}
                    className="rounded-lg px-4 cursor-pointer flex items-center gap-2"
                    style={{ height: 32, fontSize: 12, fontWeight: 500, border: '1px solid var(--accent)', color: 'var(--accent)', background: 'transparent', opacity: busy ? 0.6 : 1 }}
                    onMouseEnter={(e) => { if (!busy) e.currentTarget.style.background = 'var(--accent-dim)' }}
                    onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
                  >
                    {busy === 'export' ? spinner : <IconCheck size={13} />}
                    {busy === 'export' ? 'Exporting…' : 'Export to Drive'}
                  </button>
                  <button
                    onClick={() => setConfirmingRestore(true)}
                    disabled={busy !== null}
                    className="rounded-lg px-4 cursor-pointer flex items-center gap-2"
                    style={{ height: 32, fontSize: 12, fontWeight: 500, color: 'var(--red)', background: 'transparent', border: '1px solid var(--border-soft)', opacity: busy ? 0.6 : 1 }}
                    onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--red)')}
                    onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-soft)')}
                  >
                    {busy === 'restore' ? spinner : <IconRestore size={13} />}
                    {busy === 'restore' ? 'Restoring…' : 'Restore from Drive…'}
                  </button>
                </div>
              </div>
            ) : (
              <div
                className="rounded-lg px-4 py-3 flex flex-col gap-2"
                style={{ background: 'rgba(224,82,82,0.08)', border: '1px solid rgba(224,82,82,0.2)' }}
              >
                <p className="text-xs" style={{ color: 'var(--red)', lineHeight: 1.5 }}>
                  Restoring replaces ALL bookmarks, folders and tags on this device with the
                  Drive backup. A safety export of the current data is written beside the
                  database first. If Neon sync is enabled, the next sync merges the synced
                  state back in.
                </p>
                <div className="flex gap-2">
                  <button
                    onClick={restoreFromDrive}
                    className="rounded-lg px-4 cursor-pointer"
                    style={{ height: 30, fontSize: 12, fontWeight: 600, background: 'var(--red)', color: '#fff', border: 'none' }}
                  >
                    Replace local data
                  </button>
                  <button
                    onClick={() => setConfirmingRestore(false)}
                    className="rounded-lg px-3 cursor-pointer"
                    style={{ height: 30, fontSize: 12, fontWeight: 500, ...inputStyle }}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </SettingsLayout>
  )
}
