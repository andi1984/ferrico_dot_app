import { render, screen, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { ContextMenu } from './ContextMenu'
import type { CtxMenuState } from './ContextMenu'

const baseState: CtxMenuState = {
  x: 100,
  y: 100,
  items: [
    { label: 'Open', action: vi.fn() },
    { label: 'Delete', action: vi.fn(), danger: true },
  ],
}

describe('ContextMenu', () => {
  it('renders all items', () => {
    render(<ContextMenu state={baseState} onClose={() => {}} />)
    expect(screen.getByText('Open')).toBeInTheDocument()
    expect(screen.getByText('Delete')).toBeInTheDocument()
  })

  it('calls the item action and onClose when an item is clicked', async () => {
    const action = vi.fn()
    const onClose = vi.fn()
    const state: CtxMenuState = { x: 100, y: 100, items: [{ label: 'Open', action }] }
    render(<ContextMenu state={state} onClose={onClose} />)
    await userEvent.click(screen.getByText('Open'))
    expect(action).toHaveBeenCalledTimes(1)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose when Escape is pressed', () => {
    const onClose = vi.fn()
    render(<ContextMenu state={baseState} onClose={onClose} />)
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose when clicking outside the menu', () => {
    const onClose = vi.fn()
    render(
      <div>
        <ContextMenu state={baseState} onClose={onClose} />
        <button>Outside</button>
      </div>,
    )
    fireEvent.mouseDown(screen.getByText('Outside'))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('does not call onClose when clicking inside the menu', () => {
    const onClose = vi.fn()
    render(<ContextMenu state={baseState} onClose={onClose} />)
    fireEvent.mouseDown(screen.getByRole('menu'))
    expect(onClose).not.toHaveBeenCalled()
  })

  it('renders separator for items with sep flag', () => {
    const state: CtxMenuState = {
      x: 0,
      y: 0,
      items: [
        { label: 'A', action: vi.fn() },
        { sep: true, label: '', action: () => {} },
        { label: 'B', action: vi.fn() },
      ],
    }
    render(<ContextMenu state={state} onClose={() => {}} />)
    expect(screen.getByRole('separator')).toBeInTheDocument()
  })

  it('applies danger class to danger items', () => {
    const state: CtxMenuState = {
      x: 0,
      y: 0,
      items: [{ label: 'Delete', action: vi.fn(), danger: true }],
    }
    render(<ContextMenu state={state} onClose={() => {}} />)
    expect(screen.getByText('Delete')).toHaveClass('danger')
  })
})
