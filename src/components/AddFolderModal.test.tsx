import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { AddFolderModal } from './AddFolderModal'

describe('AddFolderModal', () => {
  it('renders the name field', () => {
    render(<AddFolderModal onAdd={() => {}} onClose={() => {}} />)
    expect(screen.getByLabelText(/name/i)).toBeInTheDocument()
  })

  it('does not call onAdd when the name is empty', async () => {
    const onAdd = vi.fn()
    render(<AddFolderModal onAdd={onAdd} onClose={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: /create folder/i }))
    expect(onAdd).not.toHaveBeenCalled()
  })

  it('calls onAdd with the trimmed name on submit', async () => {
    const onAdd = vi.fn()
    render(<AddFolderModal onAdd={onAdd} onClose={() => {}} />)
    await userEvent.type(screen.getByLabelText(/name/i), '  Work  ')
    await userEvent.click(screen.getByRole('button', { name: /create folder/i }))
    expect(onAdd).toHaveBeenCalledWith('Work')
  })
})
