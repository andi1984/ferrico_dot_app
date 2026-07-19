import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { MobileHeader } from './MobileHeader'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'

const PAIRED = { enabled: true, last_sync: '2026-07-18T10:00:00Z' }

function mockBackupStatus(status: unknown) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === 'backup_status') return Promise.resolve(status)
    return Promise.resolve(null)
  })
}

function makeProps(overrides: Partial<Parameters<typeof MobileHeader>[0]> = {}) {
  return {
    onSearch: vi.fn(),
    viewMode: 'list' as const,
    onToggleView: vi.fn(),
    theme: 'dark' as const,
    onToggleTheme: vi.fn(),
    onOpenSettings: vi.fn(),
    syncing: false,
    ...overrides,
  }
}

describe('MobileHeader', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockBackupStatus(null)
  })

  it('forwards the debounced search query and hides the ⌘F hint', async () => {
    const props = makeProps()
    render(<MobileHeader {...props} />)
    expect(screen.queryByText('⌘F')).not.toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Search bookmarks'), { target: { value: 'rust' } })
    await waitFor(() => expect(props.onSearch).toHaveBeenCalledWith('rust'))
  })

  it('toggles the view mode and opens settings', () => {
    const props = makeProps()
    render(<MobileHeader {...props} />)
    fireEvent.click(screen.getByRole('button', { name: 'Switch to grid view' }))
    expect(props.onToggleView).toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }))
    expect(props.onOpenSettings).toHaveBeenCalled()
  })

  it('renders the filter button only when a handler is provided', () => {
    const { unmount } = render(<MobileHeader {...makeProps()} />)
    expect(screen.queryByRole('button', { name: 'Filter by folder or tag' })).not.toBeInTheDocument()
    unmount()

    const onOpenFilter = vi.fn()
    render(<MobileHeader {...makeProps({ onOpenFilter })} />)
    fireEvent.click(screen.getByRole('button', { name: 'Filter by folder or tag' }))
    expect(onOpenFilter).toHaveBeenCalled()
  })

  it('hides refresh and the last-sync line while not paired', async () => {
    mockBackupStatus({ enabled: false, last_sync: null })
    render(<MobileHeader {...makeProps()} />)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('backup_status'))
    expect(screen.queryByRole('button', { name: 'Refresh bookmarks' })).not.toBeInTheDocument()
    expect(screen.queryByText(/Last sync:/)).not.toBeInTheDocument()
  })

  it('shows refresh and the last-sync line when paired', async () => {
    mockBackupStatus(PAIRED)
    render(<MobileHeader {...makeProps()} />)
    expect(await screen.findByRole('button', { name: 'Refresh bookmarks' })).toBeEnabled()
    expect(screen.getByText(/Last sync:/)).not.toHaveTextContent('never')
  })

  it('shows "never" when paired but no sync has completed yet', async () => {
    mockBackupStatus({ enabled: true, last_sync: null })
    render(<MobileHeader {...makeProps()} />)
    expect(await screen.findByText('Last sync: never')).toBeInTheDocument()
  })

  it('invokes backup_sync_now on refresh tap', async () => {
    mockBackupStatus(PAIRED)
    render(<MobileHeader {...makeProps()} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Refresh bookmarks' }))
    expect(invoke).toHaveBeenCalledWith('backup_sync_now')
  })

  it('disables refresh and shows the spinner while syncing', async () => {
    mockBackupStatus(PAIRED)
    const { rerender } = render(<MobileHeader {...makeProps()} />)
    await screen.findByRole('button', { name: 'Refresh bookmarks' })

    rerender(<MobileHeader {...makeProps({ syncing: true })} />)
    expect(screen.getByRole('button', { name: 'Refresh bookmarks' })).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent('Syncing…')
  })

  it('refetches backup_status after a sync cycle ends', async () => {
    mockBackupStatus(PAIRED)
    const { rerender } = render(<MobileHeader {...makeProps()} />)
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('backup_status'))
    const callsBefore = vi.mocked(invoke).mock.calls.filter((c) => c[0] === 'backup_status').length

    rerender(<MobileHeader {...makeProps({ syncing: true })} />)
    rerender(<MobileHeader {...makeProps({ syncing: false })} />)
    await waitFor(() => {
      const calls = vi.mocked(invoke).mock.calls.filter((c) => c[0] === 'backup_status').length
      expect(calls).toBe(callsBefore + 1)
    })
  })
})
