import { render, screen, waitFor, fireEvent, act } from '@testing-library/react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { MobileApp, FOREGROUND_SYNC_MIN_INTERVAL_MS } from './MobileApp'
import { makeBookmark, makeFolder, makeTag } from '../test-utils'
import type { Bookmark, SidebarData } from '../types'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('../events', () => ({
  subscribeToBookmarkAdded: vi.fn(),
  subscribeToCoverUpdated: vi.fn(),
  subscribeToBackupSync: vi.fn(),
}))

// happy-dom returns 0 for clientHeight, which would make the grid/list
// virtualizer render zero items — mock it to render every item (see
// BookmarkList.test.tsx).
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count, estimateSize }: { count: number; estimateSize: (i: number) => number }) => ({
    getTotalSize: () => Array.from({ length: count }, (_, i) => estimateSize(i)).reduce((a, b) => a + b, 0),
    getVirtualItems: () => Array.from({ length: count }, (_, i) => ({
      key: i,
      index: i,
      start: Array.from({ length: i }, (_, j) => estimateSize(j)).reduce((a, b) => a + b, 0),
      size: estimateSize(i),
    })),
    measureElement: () => {},
  }),
}))

import { invoke } from '@tauri-apps/api/core'
import { subscribeToBackupSync, subscribeToBookmarkAdded, subscribeToCoverUpdated } from '../events'

function makeSidebar(overrides?: Partial<SidebarData>): SidebarData {
  return {
    folders: [makeFolder({ id: 'folder-1', name: 'Reading' })],
    tags: [makeTag({ id: 'tag-1', name: 'rust' })],
    counts: { total: 2, inbox: 0, bin: 0, broken: 0 },
    ...overrides,
  }
}

function mockBackend({
  bookmarks = [
    makeBookmark({ id: 'bm-1', title: 'Example', url: 'https://example.com' }),
    makeBookmark({ id: 'bm-2', title: 'Rust Blog', url: 'https://blog.rust-lang.org/post' }),
  ],
  sidebar = makeSidebar(),
}: { bookmarks?: Bookmark[]; sidebar?: SidebarData } = {}) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === 'get_bookmarks') return Promise.resolve(bookmarks)
    if (cmd === 'get_bin_bookmarks') return Promise.resolve([
      makeBookmark({ id: 'bm-binned', title: 'Binned', url: 'https://binned.example.com' }),
    ])
    if (cmd === 'get_sidebar') return Promise.resolve(sidebar)
    if (cmd === 'neon_status') return Promise.resolve({
      configured: false, enabled: false, host: null, dbname: 'neondb', user: null,
      interval_min: 0, last_seq: 0, last_sync: null,
    })
    return Promise.resolve(null)
  })
}

describe('MobileApp shell', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    vi.mocked(subscribeToBackupSync).mockResolvedValue(() => {})
    vi.mocked(subscribeToCoverUpdated).mockResolvedValue(() => {})
  })

  it('shows the loading skeleton while the first load is pending', () => {
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}))
    render(<MobileApp />)
    expect(screen.getByLabelText('Loading bookmarks')).toBeInTheDocument()
  })

  it('loads bookmarks and sidebar on mount', async () => {
    mockBackend()
    render(<MobileApp />)
    expect(await screen.findByText('Example')).toBeInTheDocument()
    expect(screen.getByText('Rust Blog')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('get_bookmarks', {
      folderId: null,
      tagId: null,
      search: null,
      inboxOnly: false,
    })
    expect(invoke).toHaveBeenCalledWith('get_sidebar')
  })

  it('purges the expired bin on startup but never subscribes to bookmark-added', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    const commands = vi.mocked(invoke).mock.calls.map((c) => c[0])
    expect(commands).toContain('purge_expired_bin')
    // The extension HTTP server doesn't run on mobile, so there is no
    // bookmark-added event to listen for.
    expect(subscribeToBookmarkAdded).not.toHaveBeenCalled()
  })

  it('applies the stored theme to the document root', async () => {
    localStorage.setItem('ferrico:theme', 'light')
    mockBackend()
    render(<MobileApp />)
    await waitFor(() => {
      expect(document.documentElement.getAttribute('data-theme')).toBe('light')
    })
  })

  it('shows an empty-library message when there are no bookmarks', async () => {
    mockBackend({ bookmarks: [], sidebar: makeSidebar({ counts: { total: 0, inbox: 0, bin: 0, broken: 0 } }) })
    render(<MobileApp />)
    expect(await screen.findByText('Your library is empty')).toBeInTheDocument()
  })

  it('refetches with the folder filter when a folder is picked from the FilterDrawer', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Filter by folder or tag' }))
    fireEvent.click(screen.getByRole('button', { name: /^Reading/ }))
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('get_bookmarks', {
        folderId: 'folder-1',
        tagId: null,
        search: null,
        inboxOnly: false,
      })
    })
    // Picking a filter closes the drawer.
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('refetches with the search term when the search input changes', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.change(screen.getByLabelText('Search bookmarks'), { target: { value: 'rust' } })
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('get_bookmarks', {
        folderId: null,
        tagId: null,
        search: 'rust',
        inboxOnly: false,
      })
    })
  })

  it('reloads bookmarks and sidebar when a backup sync reports changes', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    const handlers = vi.mocked(subscribeToBackupSync).mock.calls[0][0]
    const callsBefore = vi.mocked(invoke).mock.calls.length

    act(() => { handlers.onSynced?.({ op: 'pull', changed: false }) })
    expect(vi.mocked(invoke).mock.calls.length).toBe(callsBefore)

    act(() => { handlers.onSynced?.({ op: 'pull', changed: true }) })
    await waitFor(() => {
      const commands = vi.mocked(invoke).mock.calls.slice(callsBefore).map((c) => c[0])
      expect(commands).toContain('get_bookmarks')
      expect(commands).toContain('get_sidebar')
    })
  })

  it('shows a sync indicator while a backup sync is running', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    const handlers = vi.mocked(subscribeToBackupSync).mock.calls[0][0]
    act(() => { handlers.onSyncing?.({ op: 'pull' }) })
    expect(screen.getByRole('status')).toHaveTextContent('Syncing…')
    act(() => { handlers.onSynced?.({ op: 'pull', changed: false }) })
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })

  it('patches cover_url into loaded rows on cover-updated', async () => {
    localStorage.setItem('ferrico:mobile:viewMode', 'grid')
    mockBackend({ bookmarks: [makeBookmark({ id: 'bm-1', title: 'Example', cover_url: null })] })
    const { container } = render(<MobileApp />)
    await screen.findByText('Example')
    expect(container.querySelector('img[aria-hidden="true"]')).toBeNull()

    const handler = vi.mocked(subscribeToCoverUpdated).mock.calls[0][0]
    act(() => { handler({ id: 'bm-1', cover_url: 'https://example.com/cover.png' }) })
    const img = container.querySelector<HTMLImageElement>('img[aria-hidden="true"]')
    expect(img).not.toBeNull()
    expect(img!.src).toBe('https://example.com/cover.png')
  })

  it('opens a bookmark via open_url on tap', async () => {
    mockBackend()
    render(<MobileApp />)
    fireEvent.click(await screen.findByText('Example'))
    expect(invoke).toHaveBeenCalledWith('open_url', { url: 'https://example.com' })
  })

  it('navigates to the settings screen and back', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(await screen.findByLabelText('Pairing code')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(await screen.findByText('Example')).toBeInTheDocument()
  })
})

describe('MobileApp write flows', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    vi.mocked(subscribeToBackupSync).mockResolvedValue(() => {})
    vi.mocked(subscribeToCoverUpdated).mockResolvedValue(() => {})
  })

  it('adds a bookmark through the FAB modal', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Add bookmark' }))
    fireEvent.change(screen.getByLabelText('URL *'), { target: { value: 'https://new.example.com' } })
    fireEvent.change(screen.getByLabelText('Title *'), { target: { value: 'New one' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save bookmark' }))
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('add_bookmark', {
        input: expect.objectContaining({ url: 'https://new.example.com', title: 'New one' }),
      })
    })
  })

  it('moves a bookmark to the bin via the row action sheet', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Example' }))
    fireEvent.click(screen.getByRole('button', { name: /Move to bin/ }))
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('delete_bookmark', { id: 'bm-1' })
    })
  })

  it('moves a bookmark to a folder via the folder picker', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Example' }))
    fireEvent.click(screen.getByRole('button', { name: /Move to folder/ }))
    fireEvent.click(await screen.findByRole('button', { name: /^Reading/ }))
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('move_bookmark', { id: 'bm-1', folderId: 'folder-1' })
    })
  })

  it('bin view lists binned bookmarks and restores via the action sheet', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Filter by folder or tag' }))
    fireEvent.click(screen.getByRole('button', { name: /^Bin/ }))
    expect(await screen.findByText('Binned')).toBeInTheDocument()
    expect(invoke).toHaveBeenCalledWith('get_bin_bookmarks')
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Binned' }))
    fireEvent.click(screen.getByRole('button', { name: /Restore/ }))
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('restore_bookmark', { id: 'bm-binned' })
    })
  })

  it('creates a folder from the FilterDrawer', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Filter by folder or tag' }))
    fireEvent.click(screen.getByRole('button', { name: 'New folder' }))
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'Later' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create folder' }))
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('add_folder', { name: 'Later', parentId: null })
    })
  })

  it('deletes a tag through its action sheet', async () => {
    mockBackend()
    render(<MobileApp />)
    await screen.findByText('Example')
    fireEvent.click(screen.getByRole('button', { name: 'Filter by folder or tag' }))
    fireEvent.click(screen.getByRole('button', { name: 'Actions for tag rust' }))
    fireEvent.click(await screen.findByRole('button', { name: /Delete tag/ }))
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('delete_tag', { id: 'tag-1' })
    })
  })
})

describe('MobileApp foreground resume sync', () => {
  let now: number
  let dateNowSpy: ReturnType<typeof vi.spyOn>

  function setVisibility(state: 'visible' | 'hidden') {
    Object.defineProperty(document, 'visibilityState', { value: state, configurable: true })
  }

  function resume() {
    document.dispatchEvent(new Event('visibilitychange'))
  }

  beforeEach(() => {
    vi.clearAllMocks()
    localStorage.clear()
    document.documentElement.removeAttribute('data-theme')
    vi.mocked(subscribeToBackupSync).mockResolvedValue(() => {})
    vi.mocked(subscribeToCoverUpdated).mockResolvedValue(() => {})
    now = 1_700_000_000_000
    dateNowSpy = vi.spyOn(Date, 'now').mockImplementation(() => now)
    setVisibility('visible')
  })

  afterEach(() => {
    dateNowSpy.mockRestore()
    setVisibility('visible')
  })

  function mockBackendWithBackupStatus(enabled: boolean) {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'get_bookmarks') return Promise.resolve([])
      if (cmd === 'get_sidebar') return Promise.resolve(makeSidebar())
      if (cmd === 'neon_status') return Promise.resolve({ enabled })
      return Promise.resolve(null)
    })
  }

  it('syncs when the app becomes visible, is paired, and the cooldown elapsed', async () => {
    mockBackendWithBackupStatus(true)
    render(<MobileApp />)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('get_bookmarks', expect.anything()))

    now += FOREGROUND_SYNC_MIN_INTERVAL_MS + 1
    act(() => resume())

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('neon_sync_now')
    })
  })

  it('does not sync when unpaired', async () => {
    mockBackendWithBackupStatus(false)
    render(<MobileApp />)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('get_bookmarks', expect.anything()))

    now += FOREGROUND_SYNC_MIN_INTERVAL_MS + 1
    act(() => resume())

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('neon_status')
    })
    expect(invoke).not.toHaveBeenCalledWith('neon_sync_now')
  })

  it('does not sync again before the cooldown elapses', async () => {
    mockBackendWithBackupStatus(true)
    render(<MobileApp />)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('get_bookmarks', expect.anything()))

    now += FOREGROUND_SYNC_MIN_INTERVAL_MS + 1
    act(() => resume())
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('neon_sync_now'))
    const callsAfterFirst = vi.mocked(invoke).mock.calls.length

    // Well within the cooldown — a second resume right away should be a no-op.
    now += 1000
    act(() => resume())
    expect(vi.mocked(invoke).mock.calls.length).toBe(callsAfterFirst)
  })

  it('does not sync when the document is hidden', async () => {
    mockBackendWithBackupStatus(true)
    render(<MobileApp />)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('get_bookmarks', expect.anything()))

    now += FOREGROUND_SYNC_MIN_INTERVAL_MS + 1
    setVisibility('hidden')
    act(() => resume())

    // MobileHeader fetches backup_status on its own mount regardless — the
    // assertion that matters here is that a hidden document never triggers a
    // sync attempt.
    expect(invoke).not.toHaveBeenCalledWith('neon_sync_now')
  })

  it('flushes pending edits via neon_flush when the app is backgrounded', async () => {
    mockBackendWithBackupStatus(true)
    render(<MobileApp />)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('get_bookmarks', expect.anything()))

    setVisibility('hidden')
    act(() => resume())

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('neon_flush')
    })
  })
})
