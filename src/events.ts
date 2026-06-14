import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type { UnlistenFn }

export async function subscribeToBookmarkAdded(onAdded: () => void): Promise<UnlistenFn> {
  return listen('bookmark-added', onAdded)
}

export type HealthCheckProgress = { current: number; total: number }

export async function subscribeToHealthCheckProgress(
  handler: (p: HealthCheckProgress) => void,
): Promise<UnlistenFn> {
  return listen<HealthCheckProgress>('health-check-progress', (e) => handler(e.payload))
}

export type CoverScanProgress = { current: number; total: number }

export async function subscribeToCoverScanProgress(
  handler: (p: CoverScanProgress) => void,
): Promise<UnlistenFn> {
  return listen<CoverScanProgress>('cover-scan-progress', (e) => handler(e.payload))
}

export type CoverUpdated = { id: string; cover_url: string }

export async function subscribeToCoverUpdated(
  handler: (p: CoverUpdated) => void,
): Promise<UnlistenFn> {
  return listen<CoverUpdated>('cover-updated', (e) => handler(e.payload))
}

// ─── Google Drive backup sync ──────────────────────────────────────────────────

export type BackupSyncStart = { op: 'pull' | 'push' }
export type BackupSyncDone = { op: 'pull' | 'push'; changed: boolean }
export type BackupSyncError = { op: 'pull' | 'push'; message: string }

/** Subscribes to the backup lifecycle events. Returns a single unlisten that
 *  tears down all three listeners. */
export async function subscribeToBackupSync(handlers: {
  onSyncing?: (p: BackupSyncStart) => void
  onSynced?: (p: BackupSyncDone) => void
  onError?: (p: BackupSyncError) => void
}): Promise<UnlistenFn> {
  const unlistens = await Promise.all([
    listen<BackupSyncStart>('backup-syncing', (e) => handlers.onSyncing?.(e.payload)),
    listen<BackupSyncDone>('backup-synced', (e) => handlers.onSynced?.(e.payload)),
    listen<BackupSyncError>('backup-error', (e) => handlers.onError?.(e.payload)),
  ])
  return () => unlistens.forEach((fn) => fn())
}
