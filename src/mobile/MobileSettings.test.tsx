import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { MobileSettings } from './MobileSettings'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'

const PAIRING_CODE = 'ferrico-pair:v1:dGVzdC1wYXlsb2Fk'

function disconnectedStatus(overrides?: Record<string, unknown>) {
  return {
    has_credentials: false,
    connected: false,
    account_email: null,
    folder_id: null,
    folder_name: null,
    last_sync: null,
    interval_min: 0,
    enabled: false,
    ...overrides,
  }
}

function connectedStatus(overrides?: Record<string, unknown>) {
  return {
    has_credentials: true,
    connected: true,
    account_email: 'user@example.com',
    folder_id: 'folder-abc',
    folder_name: 'Ferrico Backups',
    last_sync: null,
    interval_min: 0,
    enabled: true,
    ...overrides,
  }
}

function mockBackend(initialStatus: Record<string, unknown>) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === 'backup_status') return Promise.resolve(initialStatus)
    return Promise.resolve(null)
  })
}

describe('MobileSettings', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
  })

  it('shows the pairing textarea when not connected', async () => {
    mockBackend(disconnectedStatus())
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => {
      expect(screen.getByLabelText('Pairing code')).toBeInTheDocument()
    })
    expect(screen.getByRole('button', { name: 'Pair this device' })).toBeDisabled()
  })

  it('imports a pairing code and shows the connected dashboard on success', async () => {
    mockBackend(disconnectedStatus())
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'backup_status') return Promise.resolve(disconnectedStatus())
      if (cmd === 'backup_import_pairing') return Promise.resolve(connectedStatus())
      return Promise.resolve(null)
    })
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => screen.getByLabelText('Pairing code'))

    await userEvent.type(screen.getByLabelText('Pairing code'), PAIRING_CODE)
    await userEvent.click(screen.getByRole('button', { name: 'Pair this device' }))

    expect(invoke).toHaveBeenCalledWith('backup_import_pairing', { payload: PAIRING_CODE })
    await waitFor(() => {
      expect(screen.getByText('user@example.com')).toBeInTheDocument()
    })
    expect(screen.getByText('Ferrico Backups')).toBeInTheDocument()
    expect(screen.queryByLabelText('Pairing code')).not.toBeInTheDocument()
  })

  it('surfaces an error for an invalid pairing code and keeps the textarea visible', async () => {
    mockBackend(disconnectedStatus())
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'backup_status') return Promise.resolve(disconnectedStatus())
      if (cmd === 'backup_import_pairing')
        return Promise.reject({ name: 'Validation', message: 'Invalid pairing code' })
      return Promise.resolve(null)
    })
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => screen.getByLabelText('Pairing code'))

    await userEvent.type(screen.getByLabelText('Pairing code'), 'garbage')
    await userEvent.click(screen.getByRole('button', { name: 'Pair this device' }))

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Invalid pairing code')
    })
    expect(screen.getByLabelText('Pairing code')).toHaveValue('garbage')
  })

  it('shows last sync, syncs now, and unpairs from the connected dashboard', async () => {
    mockBackend(connectedStatus({ last_sync: '2026-07-18T10:00:00Z' }))
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'backup_status') return Promise.resolve(connectedStatus({ last_sync: '2026-07-18T10:00:00Z' }))
      if (cmd === 'backup_sync_now') return Promise.resolve(connectedStatus({ last_sync: '2026-07-19T09:00:00Z' }))
      if (cmd === 'backup_disconnect') return Promise.resolve(disconnectedStatus())
      return Promise.resolve(null)
    })
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => screen.getByText('user@example.com'))
    expect(screen.getByText(/download-only/i)).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: 'Sync now' }))
    expect(invoke).toHaveBeenCalledWith('backup_sync_now')

    await userEvent.click(screen.getByRole('button', { name: 'Unpair' }))
    expect(invoke).toHaveBeenCalledWith('backup_disconnect')
    await waitFor(() => {
      expect(screen.getByLabelText('Pairing code')).toBeInTheDocument()
    })
  })

  it('toggles the theme', async () => {
    mockBackend(disconnectedStatus())
    const onToggleTheme = vi.fn()
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={onToggleTheme} />)
    await waitFor(() => screen.getByLabelText('Pairing code'))
    await userEvent.click(screen.getByRole('button', { name: 'Switch to light theme' }))
    expect(onToggleTheme).toHaveBeenCalled()
  })

  it('calls onClose via the back button', async () => {
    mockBackend(disconnectedStatus())
    const onClose = vi.fn()
    render(<MobileSettings onClose={onClose} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => screen.getByLabelText('Pairing code'))
    await userEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(onClose).toHaveBeenCalled()
  })
})
