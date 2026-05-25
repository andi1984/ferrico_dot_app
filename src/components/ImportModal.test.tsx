import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ImportModal } from './ImportModal'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'

const defaultProps = {
  onClose: vi.fn(),
  onDone: vi.fn(),
  onImportCsv: vi.fn(),
}

function makeFile(name: string, content = 'data') {
  return new File([content], name, { type: 'text/plain' })
}

beforeEach(() => {
  vi.clearAllMocks()
  global.URL.createObjectURL = vi.fn().mockReturnValue('blob:test')
  global.URL.revokeObjectURL = vi.fn()
})

describe('ImportModal', () => {
  it('renders the drop zone and all format chips', () => {
    render(<ImportModal {...defaultProps} />)
    expect(screen.getByRole('button', { name: /choose file to import/i })).toBeInTheDocument()
    expect(screen.getByText('JSON')).toBeInTheDocument()
    expect(screen.getByText('HTML')).toBeInTheDocument()
    expect(screen.getByText('OPML')).toBeInTheDocument()
    expect(screen.getByText('CSV')).toBeInTheDocument()
  })

  it('routes .csv to onImportCsv and closes modal', async () => {
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('bookmarks.csv'))
    expect(defaultProps.onImportCsv).toHaveBeenCalledOnce()
    expect(defaultProps.onClose).toHaveBeenCalledOnce()
    expect(invoke).not.toHaveBeenCalled()
  })

  it('calls import_json and shows success for .json', async () => {
    vi.mocked(invoke).mockResolvedValue({ imported: 42, errors: [] })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('bookmarks.json', '{}'))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_json', { json: '{}' }))
    await waitFor(() => expect(screen.getByText(/42 bookmarks imported/i)).toBeInTheDocument())
    expect(defaultProps.onDone).toHaveBeenCalledOnce()
  })

  it('calls import_netscape_html for .html', async () => {
    vi.mocked(invoke).mockResolvedValue({ imported: 5, errors: [] })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('bookmarks.html', '<html/>'))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_netscape_html', { html: '<html/>' }))
  })

  it('calls import_opml for .opml', async () => {
    vi.mocked(invoke).mockResolvedValue({ imported: 3, errors: [] })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('feeds.opml', '<opml/>'))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_opml', { xml: '<opml/>' }))
  })

  it('calls import_opml for .xml', async () => {
    vi.mocked(invoke).mockResolvedValue({ imported: 1, errors: [] })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('feeds.xml', '<opml/>'))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_opml', { xml: '<opml/>' }))
  })

  it('shows error message for unsupported file type dropped onto drop zone', async () => {
    render(<ImportModal {...defaultProps} />)
    const dropZone = screen.getByRole('button', { name: /choose file to import/i })
    fireEvent.drop(dropZone, { dataTransfer: { files: [makeFile('data.txt')] } })
    await waitFor(() => expect(screen.getByText(/unsupported file type: \.txt/i)).toBeInTheDocument())
    expect(invoke).not.toHaveBeenCalled()
  })

  it('shows partial errors when some rows are skipped', async () => {
    vi.mocked(invoke).mockResolvedValue({ imported: 10, errors: ['Row 3: missing url', 'Row 7: invalid url'] })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('bookmarks.json', '{}'))
    await waitFor(() => expect(screen.getByText(/2 skipped/i)).toBeInTheDocument())
    expect(screen.getByText('Row 3: missing url')).toBeInTheDocument()
  })

  it('shows error state and Try Again button on invoke failure', async () => {
    vi.mocked(invoke).mockRejectedValue({ message: 'parse error' })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('bookmarks.json', '{}'))
    await waitFor(() => expect(screen.getByText('parse error')).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /try again/i })).toBeInTheDocument()
    expect(defaultProps.onDone).not.toHaveBeenCalled()
  })

  it('Try Again resets to idle state', async () => {
    vi.mocked(invoke).mockRejectedValue({ message: 'parse error' })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('bookmarks.json', '{}'))
    await waitFor(() => screen.getByRole('button', { name: /try again/i }))
    await userEvent.click(screen.getByRole('button', { name: /try again/i }))
    expect(screen.getByRole('button', { name: /choose file to import/i })).toBeInTheDocument()
    expect(screen.queryByText('parse error')).not.toBeInTheDocument()
  })

  it('Done button calls onClose', async () => {
    vi.mocked(invoke).mockResolvedValue({ imported: 1, errors: [] })
    render(<ImportModal {...defaultProps} />)
    const input = document.querySelector('input[type="file"]') as HTMLInputElement
    await userEvent.upload(input, makeFile('bookmarks.json', '{}'))
    await waitFor(() => screen.getByRole('button', { name: /done/i }))
    await userEvent.click(screen.getByRole('button', { name: /done/i }))
    expect(defaultProps.onClose).toHaveBeenCalledOnce()
  })
})
