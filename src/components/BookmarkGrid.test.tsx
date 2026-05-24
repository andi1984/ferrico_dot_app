import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { BookmarkGrid, computeColumns } from './BookmarkGrid'
import { makeBookmark } from '../test-utils'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

// happy-dom returns 0 for clientHeight, so the virtualizer would render nothing.
// Stub it to return every row, which is what we want to assert against in tests.
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count, estimateSize }: { count: number; estimateSize: (i: number) => number }) => ({
    getTotalSize: () => count * estimateSize(0),
    getVirtualItems: () =>
      Array.from({ length: count }, (_, i) => ({
        key: i,
        index: i,
        start: i * estimateSize(i),
        size: estimateSize(i),
      })),
    measureElement: () => {},
  }),
}))

describe('computeColumns', () => {
  it('returns 1 column for tiny widths', () => {
    expect(computeColumns(0)).toBe(1)
    expect(computeColumns(100)).toBe(1)
    expect(computeColumns(259)).toBe(1)
  })

  it('returns 1 column at exactly card min width', () => {
    expect(computeColumns(220)).toBe(1)
  })

  it('returns 2 columns when there is room for two cards plus a gap', () => {
    // 2 * 220 + 14 = 454
    expect(computeColumns(454)).toBe(2)
    expect(computeColumns(453)).toBe(1)
  })

  it('returns 4 columns for a typical desktop width', () => {
    // 4 * 220 + 3 * 14 = 922
    expect(computeColumns(922)).toBe(4)
  })

  it('grows with width', () => {
    expect(computeColumns(1600)).toBeGreaterThanOrEqual(5)
  })
})

describe('BookmarkGrid', () => {
  it('renders nothing when there are no bookmarks', () => {
    const { container } = render(
      <BookmarkGrid bookmarks={[]} onDelete={() => {}} onContext={() => {}} />,
    )
    expect(container.querySelectorAll('[aria-label^="Delete"]')).toHaveLength(0)
  })

  it('renders all bookmarks when the virtualizer reports every row', () => {
    const bookmarks = Array.from({ length: 12 }, (_, i) =>
      makeBookmark({ id: `bm-${i}`, title: `Bookmark ${i}`, url: `https://example.com/${i}` }),
    )
    render(<BookmarkGrid bookmarks={bookmarks} onDelete={() => {}} onContext={() => {}} />)
    for (let i = 0; i < 12; i++) {
      expect(screen.getByText(`Bookmark ${i}`)).toBeInTheDocument()
    }
  })

  it('uses CSS Grid for layout (not multi-column) so rows can be virtualized', () => {
    const bookmarks = [makeBookmark()]
    const { container } = render(
      <BookmarkGrid bookmarks={bookmarks} onDelete={() => {}} onContext={() => {}} />,
    )
    const rows = container.querySelectorAll('[data-grid-row]')
    expect(rows.length).toBeGreaterThan(0)
    expect((rows[0] as HTMLElement).style.display).toBe('grid')
    expect((rows[0] as HTMLElement).style.gridTemplateColumns).toMatch(/repeat\(/)
  })

  it('calls onDelete with the bookmark id when the close button is clicked', () => {
    const onDelete = vi.fn()
    const bm = makeBookmark({ id: 'bm-1', title: 'Test' })
    render(<BookmarkGrid bookmarks={[bm]} onDelete={onDelete} onContext={() => {}} />)
    screen.getByRole('button', { name: 'Delete Test' }).click()
    expect(onDelete).toHaveBeenCalledWith('bm-1')
  })

  it('calls onContext on right-click', () => {
    const onContext = vi.fn()
    const bm = makeBookmark({ id: 'bm-1', title: 'Test' })
    render(<BookmarkGrid bookmarks={[bm]} onDelete={() => {}} onContext={onContext} />)
    const link = screen.getByRole('link', { name: 'Test' })
    link.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }))
    expect(onContext).toHaveBeenCalledWith(expect.any(Object), bm)
  })
})
