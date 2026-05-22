import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}))

import { listen } from '@tauri-apps/api/event'
import { subscribeToBookmarkAdded } from './events'

describe('subscribeToBookmarkAdded', () => {
  beforeEach(() => {
    vi.mocked(listen).mockReset()
  })

  it('registers a listener for the bookmark-added event', async () => {
    vi.mocked(listen).mockResolvedValue(() => {})

    const onAdded = vi.fn()
    await subscribeToBookmarkAdded(onAdded)

    expect(listen).toHaveBeenCalledWith('bookmark-added', onAdded)
  })

  it('returns the unlisten function from listen', async () => {
    const mockUnlisten = vi.fn()
    vi.mocked(listen).mockResolvedValue(mockUnlisten)

    const unlisten = await subscribeToBookmarkAdded(() => {})

    expect(unlisten).toBe(mockUnlisten)
  })

  it('invokes the callback when the event fires', async () => {
    let capturedHandler: ((event: unknown) => void) | undefined
    vi.mocked(listen).mockImplementation((_event, handler) => {
      capturedHandler = handler as (event: unknown) => void
      return Promise.resolve(() => {})
    })

    const onAdded = vi.fn()
    await subscribeToBookmarkAdded(onAdded)

    capturedHandler!({ event: 'bookmark-added', id: 1, payload: null })
    expect(onAdded).toHaveBeenCalledTimes(1)
  })
})
