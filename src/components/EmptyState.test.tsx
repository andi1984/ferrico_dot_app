import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { EmptyState } from './EmptyState'

describe('EmptyState', () => {
  it('renders the empty library message', () => {
    render(<EmptyState onAdd={() => {}} />)
    expect(screen.getByText(/your library is empty/i)).toBeInTheDocument()
  })

  it('calls onAdd when the Add Bookmark button is clicked', async () => {
    const onAdd = vi.fn()
    render(<EmptyState onAdd={onAdd} />)
    await userEvent.click(screen.getByRole('button', { name: /add bookmark/i }))
    expect(onAdd).toHaveBeenCalledTimes(1)
  })
})
