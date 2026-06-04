import { render, screen, fireEvent, act } from '@testing-library/react'
import { createRef } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { SearchBox, type SearchBoxHandle } from './SearchBox'

describe('SearchBox', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('renders the search input', () => {
    render(<SearchBox onSearch={() => {}} />)
    expect(screen.getByLabelText('Search bookmarks')).toBeInTheDocument()
  })

  it('debounces and emits only the latest value, once', () => {
    const onSearch = vi.fn()
    render(<SearchBox onSearch={onSearch} debounceMs={300} />)
    const input = screen.getByLabelText('Search bookmarks')

    fireEvent.change(input, { target: { value: 'ru' } })
    act(() => vi.advanceTimersByTime(100)) // still inside the window
    fireEvent.change(input, { target: { value: 'rust' } })
    act(() => vi.advanceTimersByTime(300)) // window elapses

    expect(onSearch).toHaveBeenCalledWith('rust')
    expect(onSearch).not.toHaveBeenCalledWith('ru') // intermediate keystroke coalesced away
  })

  it('shows a clear button that empties the field and emits the empty query', () => {
    const onSearch = vi.fn()
    render(<SearchBox onSearch={onSearch} />)
    const input = screen.getByLabelText('Search bookmarks') as HTMLInputElement

    fireEvent.change(input, { target: { value: 'docker' } })
    fireEvent.click(screen.getByLabelText('Clear search'))
    expect(input.value).toBe('')

    act(() => vi.advanceTimersByTime(300))
    expect(onSearch).toHaveBeenLastCalledWith('')
  })

  it('clears on Escape', () => {
    render(<SearchBox onSearch={() => {}} />)
    const input = screen.getByLabelText('Search bookmarks') as HTMLInputElement
    fireEvent.change(input, { target: { value: 'kernel' } })
    fireEvent.keyDown(input, { key: 'Escape' })
    expect(input.value).toBe('')
  })

  it('exposes an imperative focus() handle (for the ⌘F shortcut)', () => {
    const ref = createRef<SearchBoxHandle>()
    render(<SearchBox ref={ref} onSearch={() => {}} />)
    act(() => ref.current?.focus())
    expect(screen.getByLabelText('Search bookmarks')).toHaveFocus()
  })
})
