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
    onCreateTag: vi.fn(),
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

  it('shows a tag combobox to search existing tags', async () => {
    const tags = [makeTag({ id: 't1', name: 'Design' })]
    render(<AddBookmarkModal {...makeProps({ tags })} />)
    const combobox = screen.getByRole('combobox')
    await userEvent.click(combobox)
    expect(screen.getByRole('option', { name: /Design/ })).toBeInTheDocument()
  })

  it('selects an existing tag and includes its id in submission', async () => {
    const onAdd = vi.fn()
    const tags = [makeTag({ id: 't1', name: 'Design' })]
    render(<AddBookmarkModal {...makeProps({ tags, onAdd })} />)

    await userEvent.click(screen.getByRole('combobox'))
    await userEvent.click(screen.getByRole('option', { name: /Design/ }))

    await userEvent.type(screen.getByLabelText(/url/i), 'https://example.com')
    await userEvent.type(screen.getByLabelText(/title/i), 'Example')
    await userEvent.click(screen.getByRole('button', { name: /save bookmark/i }))
    expect(onAdd).toHaveBeenCalledWith(expect.objectContaining({ tag_ids: ['t1'] }))
  })

  it('creates a new tag inline and includes it in submission', async () => {
    const onAdd = vi.fn()
    const onCreateTag = vi.fn().mockResolvedValue(makeTag({ id: 'new1', name: 'fresh' }))
    render(<AddBookmarkModal {...makeProps({ onAdd, onCreateTag })} />)

    await userEvent.type(screen.getByRole('combobox'), 'fresh')
    await userEvent.click(screen.getByRole('option', { name: /Create/ }))
    expect(onCreateTag).toHaveBeenCalledWith('fresh', expect.any(String))

    await userEvent.type(screen.getByLabelText(/url/i), 'https://example.com')
    await userEvent.type(screen.getByLabelText(/title/i), 'Example')
    await userEvent.click(screen.getByRole('button', { name: /save bookmark/i }))
    expect(onAdd).toHaveBeenCalledWith(expect.objectContaining({ tag_ids: ['new1'] }))
  })
})
