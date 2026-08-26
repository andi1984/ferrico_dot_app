import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { MobileSettings } from './MobileSettings'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'

const PAIRING_CODE = 'ferrico-pair:v2:dGVzdC1wYXlsb2Fk'

function unpairedStatus(overrides?: Record<string, unknown>) {
  return {
    configured: false,
    enabled: false,
    host: null,
    dbname: 'neondb',
    user: null,
    interval_min: 0,
    last_seq: 0,
    last_sync: null,
    ...overrides,
  }
}

function pairedStatus(overrides?: Record<string, unknown>) {
  return {
    configured: true,
    enabled: true,
    host: 'ep-test.eu-central-1.aws.neon.tech',
    dbname: 'neondb',
    user: 'ferrico',
    interval_min: 0,
    last_seq: 42,
    last_sync: null,
    ...overrides,
  }
}

function mockBackend(initialStatus: Record<string, unknown>) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === 'neon_status') return Promise.resolve(initialStatus)
    return Promise.resolve(null)
  })
}

describe('MobileSettings', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
  })

  it('shows the pairing textarea when not paired', async () => {
    mockBackend(unpairedStatus())
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => {
      expect(screen.getByLabelText('Pairing code')).toBeInTheDocument()
    })
    expect(screen.getByRole('button', { name: 'Pair this device' })).toBeDisabled()
  })

  it('imports a pairing code and shows the paired dashboard on success', async () => {
    mockBackend(unpairedStatus())
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'neon_status') return Promise.resolve(unpairedStatus())
      if (cmd === 'backup_import_pairing') return Promise.resolve(pairedStatus())
      return Promise.resolve(null)
    })
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => screen.getByLabelText('Pairing code'))

    await userEvent.type(screen.getByLabelText('Pairing code'), PAIRING_CODE)
    await userEvent.click(screen.getByRole('button', { name: 'Pair this device' }))

    expect(invoke).toHaveBeenCalledWith('backup_import_pairing', { payload: PAIRING_CODE })
    await waitFor(() => {
      expect(screen.getByText(/ferrico@ep-test/)).toBeInTheDocument()
    })
    expect(screen.queryByLabelText('Pairing code')).not.toBeInTheDocument()
  })

  it('surfaces an error for an invalid pairing code and keeps the textarea visible', async () => {
    mockBackend(unpairedStatus())
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'neon_status') return Promise.resolve(unpairedStatus())
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

  it('shows last sync, syncs now, and unpairs from the paired dashboard', async () => {
    const t1 = Math.floor(new Date('2026-07-18T10:00:00Z').getTime() / 1000)
    const t2 = Math.floor(new Date('2026-07-19T09:00:00Z').getTime() / 1000)
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'neon_status') return Promise.resolve(pairedStatus({ last_sync: t1 }))
      if (cmd === 'neon_sync_now') return Promise.resolve(pairedStatus({ last_sync: t2 }))
      if (cmd === 'neon_disconnect') return Promise.resolve(unpairedStatus())
      return Promise.resolve(null)
    })
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => screen.getByText(/ferrico@ep-test/))
    expect(screen.getByText(/download-only/i)).toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: 'Sync now' }))
    expect(invoke).toHaveBeenCalledWith('neon_sync_now')

    await userEvent.click(screen.getByRole('button', { name: 'Unpair' }))
    expect(invoke).toHaveBeenCalledWith('neon_disconnect')
    await waitFor(() => {
      expect(screen.getByLabelText('Pairing code')).toBeInTheDocument()
    })
  })

  it('toggles the theme', async () => {
    mockBackend(unpairedStatus())
    const onToggleTheme = vi.fn()
    render(<MobileSettings onClose={() => {}} theme="dark" onToggleTheme={onToggleTheme} />)
    await waitFor(() => screen.getByLabelText('Pairing code'))
    await userEvent.click(screen.getByRole('button', { name: 'Switch to light theme' }))
    expect(onToggleTheme).toHaveBeenCalled()
  })

  it('calls onClose via the back button', async () => {
    mockBackend(unpairedStatus())
    const onClose = vi.fn()
    render(<MobileSettings onClose={onClose} theme="dark" onToggleTheme={() => {}} />)
    await waitFor(() => screen.getByLabelText('Pairing code'))
    await userEvent.click(screen.getByRole('button', { name: 'Back' }))
    expect(onClose).toHaveBeenCalled()
  })
})
