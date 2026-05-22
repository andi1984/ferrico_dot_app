import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { AddTagModal, TAG_COLORS, TAG_COLOR_NAMES } from './AddTagModal'

describe('AddTagModal', () => {
  it('renders the name field', () => {
    render(<AddTagModal onAdd={() => {}} onClose={() => {}} />)
    expect(screen.getByLabelText(/name/i)).toBeInTheDocument()
  })

  it('renders all color swatches', () => {
    render(<AddTagModal onAdd={() => {}} onClose={() => {}} />)
    for (const name of TAG_COLOR_NAMES) {
      expect(screen.getByRole('button', { name })).toBeInTheDocument()
    }
  })

  it('selects the first color by default', () => {
    render(<AddTagModal onAdd={() => {}} onClose={() => {}} />)
    expect(screen.getByRole('button', { name: TAG_COLOR_NAMES[0] })).toHaveAttribute('aria-pressed', 'true')
  })

  it('does not call onAdd when the name is empty', async () => {
    const onAdd = vi.fn()
    render(<AddTagModal onAdd={onAdd} onClose={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: /create tag/i }))
    expect(onAdd).not.toHaveBeenCalled()
  })

  it('calls onAdd with the name and the default color on submit', async () => {
    const onAdd = vi.fn()
    render(<AddTagModal onAdd={onAdd} onClose={() => {}} />)
    await userEvent.type(screen.getByLabelText(/name/i), 'Design')
    await userEvent.click(screen.getByRole('button', { name: /create tag/i }))
    expect(onAdd).toHaveBeenCalledWith('Design', TAG_COLORS[0])
  })

  it('changes the selected color and submits with the new one', async () => {
    const onAdd = vi.fn()
    render(<AddTagModal onAdd={onAdd} onClose={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: 'Blue' }))
    expect(screen.getByRole('button', { name: 'Blue' })).toHaveAttribute('aria-pressed', 'true')
    expect(screen.getByRole('button', { name: TAG_COLOR_NAMES[0] })).toHaveAttribute('aria-pressed', 'false')

    await userEvent.type(screen.getByLabelText(/name/i), 'Dev')
    await userEvent.click(screen.getByRole('button', { name: /create tag/i }))
    expect(onAdd).toHaveBeenCalledWith('Dev', TAG_COLORS[TAG_COLOR_NAMES.indexOf('Blue')])
  })
})
