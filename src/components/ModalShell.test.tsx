import { render, screen, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { ModalShell, FieldLabel, ModalActions } from './ModalShell'

describe('ModalShell', () => {
  it('renders the title and children', () => {
    render(
      <ModalShell title="Test Dialog" onClose={() => {}}>
        <p>Dialog content</p>
      </ModalShell>,
    )
    expect(screen.getByText('Test Dialog')).toBeInTheDocument()
    expect(screen.getByText('Dialog content')).toBeInTheDocument()
  })

  it('calls onClose when Escape is pressed', () => {
    const onClose = vi.fn()
    render(<ModalShell title="Test" onClose={onClose}><div /></ModalShell>)
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose when the close button is clicked', async () => {
    const onClose = vi.fn()
    render(<ModalShell title="Test" onClose={onClose}><div /></ModalShell>)
    await userEvent.click(screen.getByRole('button', { name: /close dialog/i }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose when the backdrop is clicked', () => {
    const onClose = vi.fn()
    const { container } = render(
      <ModalShell title="Test" onClose={onClose}><div /></ModalShell>,
    )
    fireEvent.click(container.firstChild as HTMLElement)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('has the correct role and aria attributes on the dialog panel', () => {
    render(<ModalShell title="My Modal" onClose={() => {}}><div /></ModalShell>)
    const dialog = screen.getByRole('dialog')
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(dialog).toHaveAttribute('aria-labelledby')
  })
})

describe('FieldLabel', () => {
  it('renders a label associated with the given htmlFor', () => {
    render(<FieldLabel htmlFor="test-input">Label Text</FieldLabel>)
    expect(screen.getByText('Label Text')).toHaveAttribute('for', 'test-input')
  })
})

describe('ModalActions', () => {
  it('renders a cancel button and a submit button with the given label', () => {
    render(<ModalActions onClose={() => {}} submitLabel="Save" />)
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /save/i })).toBeInTheDocument()
  })

  it('calls onClose when cancel is clicked', async () => {
    const onClose = vi.fn()
    render(<ModalActions onClose={onClose} submitLabel="Save" />)
    await userEvent.click(screen.getByRole('button', { name: /cancel/i }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('submit button has type="submit" so it triggers the surrounding form', () => {
    render(<ModalActions onClose={() => {}} submitLabel="Save" />)
    expect(screen.getByRole('button', { name: /save/i })).toHaveAttribute('type', 'submit')
  })
})
