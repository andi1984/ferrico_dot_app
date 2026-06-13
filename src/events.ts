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
