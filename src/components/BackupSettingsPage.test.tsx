import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { BackupSettingsPage } from './BackupSettingsPage'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('qrcode', () => ({
  default: { toDataURL: vi.fn().mockResolvedValue('data:image/png;base64,QR-TEST') },
}))

import { invoke } from '@tauri-apps/api/core'

const PAIRING_CODE = 'ferrico-pair:v1:dGVzdC1wYXlsb2Fk'

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

function mockBackend(status: Record<string, unknown>) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === 'backup_status') return Promise.resolve(status)
    if (cmd === 'backup_export_pairing') return Promise.resolve(PAIRING_CODE)
    if (cmd === 'backup_list_folders') return Promise.resolve([])
    return Promise.resolve(null)
  })
}

describe('BackupSettingsPage — mobile pairing', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
    // navigator.clipboard is a getter-only property; use defineProperty to override it
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      writable: true,
      configurable: true,
    })
  })

  it('shows the pairing section when connected with a folder selected', async () => {
    mockBackend(connectedStatus())
    render(<BackupSettingsPage onClose={() => {}} onDone={() => {}} />)
    await waitFor(() => {
      expect(screen.getByText('Pair a mobile device')).toBeInTheDocument()
    })
    expect(screen.getByRole('button', { name: /show pairing code/i })).toBeInTheDocument()
  })

  it('hides the pairing section when not connected', async () => {
    mockBackend(connectedStatus({ connected: false, account_email: null }))
    render(<BackupSettingsPage onClose={() => {}} onDone={() => {}} />)
    await waitFor(() => {
      expect(screen.getByText(/authorize ferrico/i)).toBeInTheDocument()
    })
    expect(screen.queryByText('Pair a mobile device')).not.toBeInTheDocument()
  })

  it('hides the pairing section when no folder is selected', async () => {
    mockBackend(connectedStatus({ folder_id: null, folder_name: null }))
    render(<BackupSettingsPage onClose={() => {}} onDone={() => {}} />)
    await waitFor(() => {
      expect(screen.getByText(/choose a drive folder/i)).toBeInTheDocument()
    })
    expect(screen.queryByText('Pair a mobile device')).not.toBeInTheDocument()
  })

  it('reveals the QR code and pairing string on click', async () => {
    mockBackend(connectedStatus())
    render(<BackupSettingsPage onClose={() => {}} onDone={() => {}} />)
    await waitFor(() => screen.getByRole('button', { name: /show pairing code/i }))

    await userEvent.click(screen.getByRole('button', { name: /show pairing code/i }))

    await waitFor(() => {
      expect(screen.getByAltText('Pairing QR code')).toHaveAttribute(
        'src',
        'data:image/png;base64,QR-TEST',
      )
    })
    expect(invoke).toHaveBeenCalledWith('backup_export_pairing')
    expect(screen.getByLabelText('Pairing code')).toHaveValue(PAIRING_CODE)
    expect(screen.getByText(/contains your google drive credentials/i)).toBeInTheDocument()
  })

  it('copies the pairing string to the clipboard', async () => {
    mockBackend(connectedStatus())
    render(<BackupSettingsPage onClose={() => {}} onDone={() => {}} />)
    await waitFor(() => screen.getByRole('button', { name: /show pairing code/i }))
    await userEvent.click(screen.getByRole('button', { name: /show pairing code/i }))
    await waitFor(() => screen.getByLabelText('Pairing code'))

    await userEvent.click(screen.getByRole('button', { name: /^copy$/i }))

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(PAIRING_CODE)
    expect(screen.getByText('Copied!')).toBeInTheDocument()
  })

  it('hides the code again via the Hide button', async () => {
    mockBackend(connectedStatus())
    render(<BackupSettingsPage onClose={() => {}} onDone={() => {}} />)
    await waitFor(() => screen.getByRole('button', { name: /show pairing code/i }))
    await userEvent.click(screen.getByRole('button', { name: /show pairing code/i }))
    await waitFor(() => screen.getByLabelText('Pairing code'))

    await userEvent.click(screen.getByRole('button', { name: /hide/i }))

    expect(screen.queryByLabelText('Pairing code')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /show pairing code/i })).toBeInTheDocument()
  })

  it('surfaces backend errors from the export command', async () => {
    mockBackend(connectedStatus())
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'backup_status') return Promise.resolve(connectedStatus())
      if (cmd === 'backup_export_pairing')
        return Promise.reject({ name: 'Backup', message: 'Google Drive is not connected' })
      return Promise.resolve(null)
    })
    render(<BackupSettingsPage onClose={() => {}} onDone={() => {}} />)
    await waitFor(() => screen.getByRole('button', { name: /show pairing code/i }))

    await userEvent.click(screen.getByRole('button', { name: /show pairing code/i }))

    await waitFor(() => {
      expect(screen.getByText(/google drive is not connected/i)).toBeInTheDocument()
    })
    expect(screen.queryByLabelText('Pairing code')).not.toBeInTheDocument()
  })
})
