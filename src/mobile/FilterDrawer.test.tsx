import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { FilterDrawer } from './FilterDrawer'
import { makeFolder, makeTag } from '../test-utils'
import type { MobileSelection } from './MobileApp'

const COUNTS = { total: 12, inbox: 0, bin: 0, broken: 0 }

function makeProps(overrides: Partial<Parameters<typeof FilterDrawer>[0]> = {}) {
  return {
    open: true,
    onClose: vi.fn(),
    folders: [],
    tags: [],
    counts: COUNTS,
    selection: { type: 'all' } as MobileSelection,
    onSelect: vi.fn(),
    ...overrides,
  }
}

describe('FilterDrawer', () => {
  beforeEach(() => {
    document.body.style.overflow = ''
  })

  it('renders nothing when closed', () => {
    render(<FilterDrawer {...makeProps({ open: false })} />)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('renders "All bookmarks" with the total count', () => {
    render(<FilterDrawer {...makeProps()} />)
    expect(screen.getByRole('button', { name: /All bookmarks/ })).toBeInTheDocument()
    expect(screen.getByLabelText('12 bookmarks')).toBeInTheDocument()
  })

  it('renders folders nested under their parent, with per-folder counts', () => {
    const parent = makeFolder({ id: 'f-parent', name: 'Reading', parent_id: null, bookmark_count: 3 })
    const child = makeFolder({ id: 'f-child', name: 'Rust', parent_id: 'f-parent', bookmark_count: 2 })
    render(<FilterDrawer {...makeProps({ folders: [parent, child] })} />)

    expect(screen.getByRole('button', { name: /Reading/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Rust/ })).toBeInTheDocument()
    expect(screen.getByLabelText('3 bookmarks')).toBeInTheDocument()
    expect(screen.getByLabelText('2 bookmarks')).toBeInTheDocument()
  })

  it('renders tags with their bookmark counts', () => {
    const tag = makeTag({ id: 't-1', name: 'rust', bookmark_count: 5 })
    render(<FilterDrawer {...makeProps({ tags: [tag] })} />)
    expect(screen.getByRole('button', { name: /rust/ })).toBeInTheDocument()
    expect(screen.getByLabelText('5 bookmarks')).toBeInTheDocument()
  })

  it('shows empty-state copy when there are no folders or tags', () => {
    render(<FilterDrawer {...makeProps()} />)
    expect(screen.getByText('No folders yet')).toBeInTheDocument()
    expect(screen.getByText('No tags yet')).toBeInTheDocument()
  })

  it('marks the active selection', () => {
    const folder = makeFolder({ id: 'f-1', name: 'Reading' })
    render(<FilterDrawer {...makeProps({ folders: [folder], selection: { type: 'folder', id: 'f-1' } })} />)
    expect(screen.getByRole('button', { name: /Reading/ })).toHaveAttribute('aria-current', 'page')
    expect(screen.getByRole('button', { name: /All bookmarks/ })).not.toHaveAttribute('aria-current')
  })

  it('selecting a folder fires the callback and closes the drawer', () => {
    const folder = makeFolder({ id: 'f-1', name: 'Reading' })
    const onSelect = vi.fn()
    const onClose = vi.fn()
    render(<FilterDrawer {...makeProps({ folders: [folder], onSelect, onClose })} />)
    fireEvent.click(screen.getByRole('button', { name: /Reading/ }))
    expect(onSelect).toHaveBeenCalledWith({ type: 'folder', id: 'f-1' })
    expect(onClose).toHaveBeenCalled()
  })

  it('selecting a tag fires the callback and closes the drawer', () => {
    const tag = makeTag({ id: 't-1', name: 'rust' })
    const onSelect = vi.fn()
    const onClose = vi.fn()
    render(<FilterDrawer {...makeProps({ tags: [tag], onSelect, onClose })} />)
    fireEvent.click(screen.getByRole('button', { name: /rust/ }))
    expect(onSelect).toHaveBeenCalledWith({ type: 'tag', id: 't-1' })
    expect(onClose).toHaveBeenCalled()
  })

  it('selecting "All bookmarks" fires the callback with the all selection', () => {
    const onSelect = vi.fn()
    render(<FilterDrawer {...makeProps({ onSelect })} />)
    fireEvent.click(screen.getByRole('button', { name: /All bookmarks/ }))
    expect(onSelect).toHaveBeenCalledWith({ type: 'all' })
  })

  it('closes when the backdrop is tapped', () => {
    const onClose = vi.fn()
    const { container } = render(<FilterDrawer {...makeProps({ onClose })} />)
    fireEvent.click(container.querySelector('.filter-drawer-backdrop')!)
    expect(onClose).toHaveBeenCalled()
  })

  it('closes via the close button', () => {
    const onClose = vi.fn()
    render(<FilterDrawer {...makeProps({ onClose })} />)
    fireEvent.click(screen.getByRole('button', { name: 'Close filter' }))
    expect(onClose).toHaveBeenCalled()
  })

  it('locks body scroll while open and restores it on close', () => {
    const { rerender } = render(<FilterDrawer {...makeProps({ open: true })} />)
    expect(document.body.style.overflow).toBe('hidden')
    rerender(<FilterDrawer {...makeProps({ open: false })} />)
    expect(document.body.style.overflow).toBe('')
  })
})
