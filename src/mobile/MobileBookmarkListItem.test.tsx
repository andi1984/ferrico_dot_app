import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { MobileBookmarkListItem } from './MobileBookmarkListItem'
import { makeBookmark } from '../test-utils'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

describe('MobileBookmarkListItem', () => {
  it('renders title, description and domain', () => {
    const bookmark = makeBookmark({
      title: 'Example Site',
      description: 'A short note',
      url: 'https://example.com/path',
    })
    render(<MobileBookmarkListItem bookmark={bookmark} />)
    expect(screen.getByText('Example Site')).toBeInTheDocument()
    expect(screen.getByText('A short note')).toBeInTheDocument()
    expect(screen.getByText('example.com')).toBeInTheDocument()
  })

  it('falls back to the URL when title is empty', () => {
    const bookmark = makeBookmark({ title: '', url: 'https://example.com/path' })
    render(<MobileBookmarkListItem bookmark={bookmark} />)
    expect(screen.getByText('https://example.com/path')).toBeInTheDocument()
  })

  it('tapping the row invokes open_url with the bookmark URL', () => {
    const bookmark = makeBookmark({ title: 'Test', url: 'https://example.com/foo' })
    render(<MobileBookmarkListItem bookmark={bookmark} />)
    screen.getByRole('button', { name: 'Test' }).click()
    expect(invoke).toHaveBeenCalledWith('open_url', { url: 'https://example.com/foo' })
  })

  it('suppresses the native long-press context menu', () => {
    // Android WebView (Chromium) shows a native context menu on long-press
    // unless the `contextmenu` event is preventDefault()'d — without this,
    // the touch never reaches our click handler on a real device.
    const bookmark = makeBookmark({ title: 'Test' })
    render(<MobileBookmarkListItem bookmark={bookmark} />)
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true })
    screen.getByRole('button', { name: 'Test' }).dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
  })

  it('is not user-selectable (Android long-press can select text instead of clicking)', () => {
    const bookmark = makeBookmark({ title: 'Test' })
    render(<MobileBookmarkListItem bookmark={bookmark} />)
    expect(screen.getByRole('button', { name: 'Test' }).className).toContain('select-none')
  })
})
