import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { BookmarkRow } from './BookmarkRow'
import { makeBookmark, makeTag } from '../test-utils'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

import { invoke } from '@tauri-apps/api/core'

describe('BookmarkRow', () => {
  beforeEach(() => { vi.mocked(invoke).mockClear() })

  it('renders title and domain', () => {
    render(
      <BookmarkRow
        bookmark={makeBookmark({ url: 'https://example.com', title: 'Example' })}
        onDelete={() => {}}
        onContext={() => {}}

      />,
    )
    expect(screen.getByText('Example')).toBeInTheDocument()
    expect(screen.getByText('example.com')).toBeInTheDocument()
  })

  it('renders description when present', () => {
    render(
      <BookmarkRow
        bookmark={makeBookmark({ description: 'A test note' })}
        onDelete={() => {}}
        onContext={() => {}}

      />,
    )
    expect(screen.getByText('A test note')).toBeInTheDocument()
  })

  it('does not render description when absent', () => {
    render(
      <BookmarkRow
        bookmark={makeBookmark({ description: null })}
        onDelete={() => {}}
        onContext={() => {}}

      />,
    )
    expect(screen.queryByRole('paragraph')).not.toBeInTheDocument()
  })

  it('renders up to 3 tags and an overflow count for the rest', () => {
    const tags = [
      makeTag({ id: '1', name: 'Alpha' }),
      makeTag({ id: '2', name: 'Beta' }),
      makeTag({ id: '3', name: 'Gamma' }),
      makeTag({ id: '4', name: 'Delta' }),
    ]
    render(
      <BookmarkRow
        bookmark={makeBookmark({ tags })}
        onDelete={() => {}}
        onContext={() => {}}

      />,
    )
    expect(screen.getByText('Alpha')).toBeInTheDocument()
    expect(screen.getByText('Beta')).toBeInTheDocument()
    expect(screen.getByText('Gamma')).toBeInTheDocument()
    expect(screen.queryByText('Delta')).not.toBeInTheDocument()
    expect(screen.getByText('+1')).toBeInTheDocument()
  })

  it('calls onTagClick with the tag id when a tag is clicked', () => {
    const onTagClick = vi.fn()
    render(
      <BookmarkRow
        bookmark={makeBookmark({ tags: [makeTag({ id: 'tag-7', name: 'Alpha' })] })}
        onDelete={() => {}}
        onContext={() => {}}
        onTagClick={onTagClick}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Filter by tag Alpha' }))
    expect(onTagClick).toHaveBeenCalledWith('tag-7')
  })

  it('calls onDelete with the bookmark id when delete button is clicked', () => {
    const onDelete = vi.fn()
    render(
      <BookmarkRow
        bookmark={makeBookmark({ id: 'bm-42', title: 'Example' })}
        onDelete={onDelete}
        onContext={() => {}}

      />,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Delete Example' }))
    expect(onDelete).toHaveBeenCalledWith('bm-42')
  })

  it('calls onContext with the event and bookmark on right-click', () => {
    const onContext = vi.fn()
    const bm = makeBookmark()
    render(
      <BookmarkRow
        bookmark={bm}
        onDelete={() => {}}
        onContext={onContext}

      />,
    )
    fireEvent.contextMenu(screen.getByRole('link', { name: 'Example' }))
    expect(onContext).toHaveBeenCalledWith(expect.any(Object), bm)
  })

  it('calls invoke(open_url) when the title link is clicked', () => {
    render(
      <BookmarkRow
        bookmark={makeBookmark({ url: 'https://example.com', title: 'Example' })}
        onDelete={() => {}}
        onContext={() => {}}

      />,
    )
    fireEvent.click(screen.getByRole('link', { name: 'Example' }))
    expect(invoke).toHaveBeenCalledWith('open_url', { url: 'https://example.com' })
  })
})
