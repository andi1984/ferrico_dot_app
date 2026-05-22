import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { AddBookmarkModal } from './AddBookmarkModal'
import type { AddBookmarkModalProps } from './AddBookmarkModal'
import { makeFolder, makeTag } from '../test-utils'

function makeProps(overrides?: Partial<AddBookmarkModalProps>): AddBookmarkModalProps {
  return {
    folders: [],
    tags: [],
    onAdd: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  }
}

describe('AddBookmarkModal', () => {
  it('renders URL and title fields', () => {
    render(<AddBookmarkModal {...makeProps()} />)
    expect(screen.getByLabelText(/url/i)).toBeInTheDocument()
    expect(screen.getByLabelText(/title/i)).toBeInTheDocument()
  })

  it('does not call onAdd when URL is missing', async () => {
    const onAdd = vi.fn()
    render(<AddBookmarkModal {...makeProps({ onAdd })} />)
    await userEvent.type(screen.getByLabelText(/title/i), 'Example')
    await userEvent.click(screen.getByRole('button', { name: /save bookmark/i }))
    expect(onAdd).not.toHaveBeenCalled()
  })

  it('does not call onAdd when title is missing', async () => {
    const onAdd = vi.fn()
    render(<AddBookmarkModal {...makeProps({ onAdd })} />)
    await userEvent.type(screen.getByLabelText(/url/i), 'https://example.com')
    await userEvent.click(screen.getByRole('button', { name: /save bookmark/i }))
    expect(onAdd).not.toHaveBeenCalled()
  })

  it('calls onAdd with the correct payload on submit', async () => {
    const onAdd = vi.fn()
    render(<AddBookmarkModal {...makeProps({ onAdd })} />)
    await userEvent.type(screen.getByLabelText(/url/i), 'https://example.com')
    await userEvent.type(screen.getByLabelText(/title/i), 'Example')
    await userEvent.click(screen.getByRole('button', { name: /save bookmark/i }))
    expect(onAdd).toHaveBeenCalledWith({
      url: 'https://example.com',
      title: 'Example',
      description: '',
      folder_id: null,
      tag_ids: [],
      feed_url: null,
    })
  })

  it('shows a folder selector when folders are provided', () => {
    const folders = [makeFolder({ id: 'f1', name: 'Work' })]
    render(<AddBookmarkModal {...makeProps({ folders })} />)
    expect(screen.getByLabelText(/folder/i)).toBeInTheDocument()
    expect(screen.getByText('Work')).toBeInTheDocument()
  })

  it('does not show the folder selector when no folders exist', () => {
    render(<AddBookmarkModal {...makeProps({ folders: [] })} />)
    expect(screen.queryByLabelText(/folder/i)).not.toBeInTheDocument()
  })

  it('shows tag toggle buttons when tags are provided', () => {
    const tags = [makeTag({ id: 't1', name: 'Design' })]
    render(<AddBookmarkModal {...makeProps({ tags })} />)
    expect(screen.getByRole('button', { name: 'Design' })).toBeInTheDocument()
  })

  it('toggles tag selection and includes selected tag ids in submission', async () => {
    const onAdd = vi.fn()
    const tags = [makeTag({ id: 't1', name: 'Design' })]
    render(<AddBookmarkModal {...makeProps({ tags, onAdd })} />)

    const tagBtn = screen.getByRole('button', { name: 'Design' })
    expect(tagBtn).toHaveAttribute('aria-pressed', 'false')
    await userEvent.click(tagBtn)
    expect(tagBtn).toHaveAttribute('aria-pressed', 'true')

    await userEvent.type(screen.getByLabelText(/url/i), 'https://example.com')
    await userEvent.type(screen.getByLabelText(/title/i), 'Example')
    await userEvent.click(screen.getByRole('button', { name: /save bookmark/i }))
    expect(onAdd).toHaveBeenCalledWith(expect.objectContaining({ tag_ids: ['t1'] }))
  })
})
