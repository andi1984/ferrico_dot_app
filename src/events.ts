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
