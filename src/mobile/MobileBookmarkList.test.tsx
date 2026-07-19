import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { MobileBookmarkList } from './MobileBookmarkList'
import { makeBookmark } from '../test-utils'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

// happy-dom returns 0 for clientHeight, which would make the virtualizer
// render zero items — mock it to render every row (see BookmarkList.test.tsx).
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

describe('MobileBookmarkList', () => {
  it('renders all rows for a small list', () => {
    const bookmarks = Array.from({ length: 5 }, (_, i) =>
      makeBookmark({ id: `bm-${i}`, title: `Bookmark ${i}`, url: `https://example.com/${i}` }),
    )
    render(<MobileBookmarkList bookmarks={bookmarks} />)
    for (let i = 0; i < 5; i++) {
      expect(screen.getByText(`Bookmark ${i}`)).toBeInTheDocument()
    }
  })

  it('renders nothing when bookmarks array is empty', () => {
    const { container } = render(<MobileBookmarkList bookmarks={[]} />)
    expect(container.querySelectorAll('.mobile-list-item')).toHaveLength(0)
  })
})
