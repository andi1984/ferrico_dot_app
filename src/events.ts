import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type { UnlistenFn }

export async function subscribeToBookmarkAdded(onAdded: () => void): Promise<UnlistenFn> {
  return listen('bookmark-added', onAdded)
}
