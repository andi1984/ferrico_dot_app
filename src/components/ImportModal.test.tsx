import { render, screen, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ImportModal } from './ImportModal'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

// Capture Tauri drag-drop listeners so tests can simulate native drops
const dragDropListeners: Array<(e: { payload: unknown }) => void> = []
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({
    onDragDropEvent: vi.fn((cb: (e: { payload: unknown }) => void) => {
      dragDropListeners.push(cb)
      return Promise.resolve(vi.fn()) // unlisten fn
    }),
  }),
}))

import { invoke } from '@tauri-apps/api/core'

function fireTauriDrop(path: string) {
  dragDropListeners[dragDropListeners.length - 1]?.({ payload: { type: 'drop', paths: [path] } })
}

function fireTauriEnter() {
  dragDropListeners[dragDropListeners.length - 1]?.({ payload: { type: 'enter', paths: [] } })
}

const defaultProps = {
  onClose: vi.fn(),
  onDone: vi.fn(),
  onImportCsv: vi.fn(),
}

beforeEach(() => {
  vi.clearAllMocks()
  dragDropListeners.length = 0
})

// ─── Rendering ────────────────────────────────────────────────────────────────

describe('ImportModal', () => {
  it('renders the drop zone and all format chips', () => {
    render(<ImportModal {...defaultProps} />)
    expect(screen.getByRole('button', { name: /choose file to import/i })).toBeInTheDocument()
    expect(screen.getByText('JSON')).toBeInTheDocument()
    expect(screen.getByText('HTML')).toBeInTheDocument()
    expect(screen.getByText('OPML')).toBeInTheDocument()
    expect(screen.getByText('CSV')).toBeInTheDocument()
  })

  // ─── File picker path ──────────────────────────────────────────────────────

  it('routes .csv file picker selection to onImportCsv with the file path', async () => {
    vi.mocked(invoke).mockResolvedValue('/tmp/bookmarks.csv')
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => expect(defaultProps.onImportCsv).toHaveBeenCalledOnce())
    expect(defaultProps.onImportCsv).toHaveBeenCalledWith('/tmp/bookmarks.csv')
    expect(invoke).toHaveBeenCalledWith('pick_import_file')
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  it('imports .json via file picker', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/bookmarks.json'
      if (cmd === 'read_text_file') return '{}'
      return { imported: 42, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('read_text_file', { path: '/tmp/bookmarks.json' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_json', { json: '{}' }))
    await waitFor(() => expect(screen.getByText(/42 bookmarks imported/i)).toBeInTheDocument())
    expect(defaultProps.onDone).toHaveBeenCalledOnce()
  })

  it('imports .html via file picker', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/bookmarks.html'
      if (cmd === 'read_text_file') return '<html/>'
      return { imported: 5, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_netscape_html', { html: '<html/>' }))
  })

  it('imports .opml via file picker', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/feeds.opml'
      if (cmd === 'read_text_file') return '<opml/>'
      return { imported: 3, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_opml', { xml: '<opml/>' }))
  })

  it('imports .xml via file picker', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/feeds.xml'
      if (cmd === 'read_text_file') return '<opml/>'
      return { imported: 1, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_opml', { xml: '<opml/>' }))
  })

  // ─── Tauri native drop path ───────────────────────────────────────────────

  it('registers a Tauri drag-drop listener on mount', () => {
    render(<ImportModal {...defaultProps} />)
    expect(dragDropListeners).toHaveLength(1)
  })

  it('imports .json via Tauri drop', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'read_text_file') return '{}'
      return { imported: 7, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await act(async () => { fireTauriDrop('/home/user/bookmarks.json') })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('read_text_file', { path: '/home/user/bookmarks.json' }))
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_json', { json: '{}' }))
    await waitFor(() => expect(screen.getByText(/7 bookmarks imported/i)).toBeInTheDocument())
    expect(defaultProps.onDone).toHaveBeenCalledOnce()
  })

  it('imports .html via Tauri drop', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'read_text_file') return '<html/>'
      return { imported: 2, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await act(async () => { fireTauriDrop('/tmp/export.html') })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_netscape_html', { html: '<html/>' }))
  })

  it('imports .opml via Tauri drop', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'read_text_file') return '<opml/>'
      return { imported: 4, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await act(async () => { fireTauriDrop('/tmp/feeds.opml') })
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('import_opml', { xml: '<opml/>' }))
  })

  it('routes .csv Tauri drop to onImportCsv with the file path', async () => {
    render(<ImportModal {...defaultProps} />)
    await act(async () => { fireTauriDrop('/tmp/data.csv') })
    expect(defaultProps.onImportCsv).toHaveBeenCalledOnce()
    expect(defaultProps.onImportCsv).toHaveBeenCalledWith('/tmp/data.csv')
    expect(invoke).not.toHaveBeenCalled()
  })

  it('shows error for unsupported type via Tauri drop', async () => {
    render(<ImportModal {...defaultProps} />)
    await act(async () => { fireTauriDrop('/tmp/data.txt') })
    await waitFor(() => expect(screen.getByText(/unsupported file type: \.txt/i)).toBeInTheDocument())
    expect(invoke).not.toHaveBeenCalled()
  })

  it('shows dragOver highlight on Tauri enter event', async () => {
    render(<ImportModal {...defaultProps} />)
    await act(async () => { fireTauriEnter() })
    const zone = screen.getByRole('button', { name: /choose file to import/i })
    expect(zone.style.borderColor).toContain('accent')
  })

  // ─── Error and result states ──────────────────────────────────────────────

  it('shows partial errors when some rows skipped', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/bookmarks.json'
      if (cmd === 'read_text_file') return '{}'
      return { imported: 10, errors: ['Row 3: missing url', 'Row 7: invalid url'] }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => expect(screen.getByText(/2 skipped/i)).toBeInTheDocument())
    expect(screen.getByText('Row 3: missing url')).toBeInTheDocument()
  })

  it('shows error state and Try Again on invoke failure', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/bookmarks.json'
      if (cmd === 'read_text_file') return '{}'
      throw { message: 'parse error' }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => expect(screen.getByText('parse error')).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /try again/i })).toBeInTheDocument()
    expect(defaultProps.onDone).not.toHaveBeenCalled()
  })

  it('Try Again resets to idle', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/bookmarks.json'
      if (cmd === 'read_text_file') return '{}'
      throw { message: 'parse error' }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => screen.getByRole('button', { name: /try again/i }))
    await userEvent.click(screen.getByRole('button', { name: /try again/i }))
    expect(screen.getByRole('button', { name: /choose file to import/i })).toBeInTheDocument()
    expect(screen.queryByText('parse error')).not.toBeInTheDocument()
  })

  it('Done button calls onClose', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'pick_import_file') return '/tmp/bookmarks.json'
      if (cmd === 'read_text_file') return '{}'
      return { imported: 1, errors: [] }
    })
    render(<ImportModal {...defaultProps} />)
    await userEvent.click(screen.getByRole('button', { name: /choose file to import/i }))
    await waitFor(() => screen.getByRole('button', { name: /done/i }))
    await userEvent.click(screen.getByRole('button', { name: /done/i }))
    expect(defaultProps.onClose).toHaveBeenCalledOnce()
  })
})
