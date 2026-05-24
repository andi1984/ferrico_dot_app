import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { BookmarkList } from './BookmarkList'
import { makeBookmark } from '../test-utils'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

// TanStack Virtual relies on element.clientHeight for windowing — happy-dom
// returns 0 for all heights, so the virtualizer renders zero items. Mock it
// to always return a flat list of all items, which is the correct behaviour
// for an infinitely tall container and is what we want to test against.
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count, estimateSize }: { count: number; estimateSize: (i: number) => number }) => ({
    getTotalSize: () => Array.from({ length: count }, (_, i) => estimateSize(i)).reduce((a, b) => a + b, 0),
    getVirtualItems: () => Array.from({ length: count }, (_, i) => ({
      key: i,
      index: i,
      start: Array.from({ length: i }, (_, j) => estimateSize(j)).reduce((a, b) => a + b, 0),
      size: estimateSize(i),
    })),
    measureElement: () => {},
  }),
}))

describe('BookmarkList', () => {
  it('renders nothing when bookmarks array is empty', () => {
    const { container } = render(
      <BookmarkList bookmarks={[]} onDelete={() => {}} onContext={() => {}} />,
    )
    expect(container.querySelectorAll('[aria-label^="Delete"]')).toHaveLength(0)
  })

  it('renders all rows for a small list', () => {
    const bookmarks = Array.from({ length: 5 }, (_, i) =>
      makeBookmark({ id: `bm-${i}`, title: `Bookmark ${i}`, url: `https://example.com/${i}` }),
    )
    render(<BookmarkList bookmarks={bookmarks} onDelete={() => {}} onContext={() => {}} />)
    for (let i = 0; i < 5; i++) {
      expect(screen.getByText(`Bookmark ${i}`)).toBeInTheDocument()
    }
  })

  it('renders taller total height for rows with description', () => {
    const bm = makeBookmark({ description: 'A note' })
    const { container } = render(
      <BookmarkList bookmarks={[bm]} onDelete={() => {}} onContext={() => {}} />,
    )
    // Total size = 1 date-group header (38px) + 1 description row (88px) = 126px.
    const inner = container.querySelector('div > div > div') as HTMLElement
    expect(inner.style.height).toBe('126px')
  })

  it('calls onDelete with the bookmark id', () => {
    const onDelete = vi.fn()
    const bm = makeBookmark({ id: 'bm-1', title: 'Test' })
    render(<BookmarkList bookmarks={[bm]} onDelete={onDelete} onContext={() => {}} />)
    screen.getByRole('button', { name: 'Delete Test' }).click()
    expect(onDelete).toHaveBeenCalledWith('bm-1')
  })

  it('calls onContext on right-click', () => {
    const onContext = vi.fn()
    const bm = makeBookmark({ id: 'bm-1', title: 'Test' })
    render(<BookmarkList bookmarks={[bm]} onDelete={() => {}} onContext={onContext} />)
    const link = screen.getByRole('link', { name: 'Test' })
    link.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }))
    expect(onContext).toHaveBeenCalledWith(expect.any(Object), bm)
  })
})
