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

export type CoverUpdated = { id: string; cover_url: string }

export async function subscribeToCoverUpdated(
  handler: (p: CoverUpdated) => void,
): Promise<UnlistenFn> {
  return listen<CoverUpdated>('cover-updated', (e) => handler(e.payload))
}

// ─── Sync lifecycle (Neon sync cycles + manual Drive export/restore) ───────────

/** Which operation a sync event belongs to. Neon cycles: `pull` (on open /
 *  mobile), `push` (before close), `auto` (change-driven), `interval`, `sync`
 *  (manual). Drive manual ops: `export`, `restore`. */
export type SyncOp = 'pull' | 'push' | 'auto' | 'interval' | 'sync' | 'export' | 'restore'

export type BackupSyncStart = { op: SyncOp }
export type BackupSyncDone = { op: SyncOp; changed: boolean; pushed?: number }
export type BackupSyncError = { op: SyncOp; message: string }

/** Subscribes to the sync lifecycle events. Returns a single unlisten that
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
