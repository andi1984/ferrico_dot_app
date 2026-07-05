import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { SettingsPage } from './SettingsPage'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'

describe('SettingsPage', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'get_api_token') return Promise.resolve('test-token-123')
      if (cmd === 'export_opml') return Promise.resolve('<opml/>')
      return Promise.resolve(null)
    })
    // navigator.clipboard is a getter-only property; use defineProperty to override it
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
      writable: true,
      configurable: true,
    })
    global.URL.createObjectURL = vi.fn().mockReturnValue('blob:test')
    global.URL.revokeObjectURL = vi.fn()
  })

  it('loads and displays the API token', async () => {
    render(<SettingsPage onClose={() => {}} onClear={() => {}} onDone={() => {}} onImportCsv={() => {}} onDeduplicate={() => {}} onBackup={() => {}} />)
    await waitFor(() => {
      expect(screen.getByText('test-token-123')).toBeInTheDocument()
    })
  })

  it('shows a placeholder until the token loads', () => {
    vi.mocked(invoke).mockReturnValue(new Promise(() => {})) // never resolves
    render(<SettingsPage onClose={() => {}} onClear={() => {}} onDone={() => {}} onImportCsv={() => {}} onDeduplicate={() => {}} onBackup={() => {}} />)
    expect(screen.getByText('…')).toBeInTheDocument()
  })

  it('copies the token to clipboard when Copy is clicked', async () => {
    render(<SettingsPage onClose={() => {}} onClear={() => {}} onDone={() => {}} onImportCsv={() => {}} onDeduplicate={() => {}} onBackup={() => {}} />)
    await waitFor(() => screen.getByText('test-token-123'))
    await userEvent.click(screen.getByRole('button', { name: /copy/i }))
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('test-token-123')
  })

  it('shows "Copied!" feedback immediately after copying', async () => {
    render(<SettingsPage onClose={() => {}} onClear={() => {}} onDone={() => {}} onImportCsv={() => {}} onDeduplicate={() => {}} onBackup={() => {}} />)
    await waitFor(() => screen.getByText('test-token-123'))
    await userEvent.click(screen.getByRole('button', { name: /copy/i }))
    expect(screen.getByText('Copied!')).toBeInTheDocument()
  })

  it('calls invoke(export_opml) when the export button is clicked', async () => {
    render(<SettingsPage onClose={() => {}} onClear={() => {}} onDone={() => {}} onImportCsv={() => {}} onDeduplicate={() => {}} onBackup={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: /^opml$/i }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('export_opml'))
  })
})
