import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi } from 'vitest'
import { Sidebar } from './Sidebar'
import type { SidebarProps } from './Sidebar'
import { makeFolder, makeTag } from '../test-utils'

function makeProps(overrides?: Partial<SidebarProps>): SidebarProps {
  return {
    folders: [],
    tags: [],
    selection: { type: 'all' },
    bookmarkCount: 0,
    onSelect: vi.fn(),
    onAddFolder: vi.fn(),
    onDeleteFolder: vi.fn(),
    onAddTag: vi.fn(),
    onDeleteTag: vi.fn(),
    onOpenSettings: vi.fn(),
    onFolderContext: vi.fn(),
    onTagContext: vi.fn(),
    ...overrides,
  }
}

describe('Sidebar', () => {
  it('renders All Bookmarks with the total count', () => {
    render(<Sidebar {...makeProps({ bookmarkCount: 42 })} />)
    expect(screen.getByText('All Bookmarks')).toBeInTheDocument()
    expect(screen.getByText('42')).toBeInTheDocument()
  })

  it('marks All Bookmarks as active when selection is { type: all }', () => {
    render(<Sidebar {...makeProps({ selection: { type: 'all' } })} />)
    expect(screen.getByRole('button', { name: /all bookmarks/i })).toHaveAttribute('aria-current', 'page')
  })

  it('calls onSelect({ type: all }) when All Bookmarks is clicked', async () => {
    const onSelect = vi.fn()
    render(<Sidebar {...makeProps({ onSelect })} />)
    await userEvent.click(screen.getByRole('button', { name: /all bookmarks/i }))
    expect(onSelect).toHaveBeenCalledWith({ type: 'all' })
  })

  it('renders folder names', () => {
    const folders = [makeFolder({ id: 'f1', name: 'Work' })]
    render(<Sidebar {...makeProps({ folders })} />)
    expect(screen.getByText('Work')).toBeInTheDocument()
  })

  it('shows an empty placeholder when there are no folders', () => {
    render(<Sidebar {...makeProps({ folders: [] })} />)
    expect(screen.getByText(/no folders yet/i)).toBeInTheDocument()
  })

  it('marks a folder as active when its id matches the selection', () => {
    const folders = [makeFolder({ id: 'f1', name: 'Work' })]
    render(<Sidebar {...makeProps({ folders, selection: { type: 'folder', id: 'f1' } })} />)
    expect(screen.getByRole('button', { name: 'Work' })).toHaveAttribute('aria-current', 'page')
  })

  it('calls onSelect with the folder selection when a folder is clicked', async () => {
    const onSelect = vi.fn()
    const folders = [makeFolder({ id: 'f1', name: 'Work' })]
    render(<Sidebar {...makeProps({ folders, onSelect })} />)
    await userEvent.click(screen.getByRole('button', { name: 'Work' }))
    expect(onSelect).toHaveBeenCalledWith({ type: 'folder', id: 'f1' })
  })

  it('renders tag names', () => {
    const tags = [makeTag({ id: 't1', name: 'Design' })]
    render(<Sidebar {...makeProps({ tags })} />)
    expect(screen.getByText('Design')).toBeInTheDocument()
  })

  it('shows an empty placeholder when there are no tags', () => {
    render(<Sidebar {...makeProps({ tags: [] })} />)
    expect(screen.getByText(/no tags yet/i)).toBeInTheDocument()
  })

  it('calls onSelect with the tag selection when a tag is clicked', async () => {
    const onSelect = vi.fn()
    const tags = [makeTag({ id: 't1', name: 'Design' })]
    render(<Sidebar {...makeProps({ tags, onSelect })} />)
    await userEvent.click(screen.getByRole('button', { name: 'Design' }))
    expect(onSelect).toHaveBeenCalledWith({ type: 'tag', id: 't1' })
  })

  it('calls onOpenSettings when the settings button is clicked', async () => {
    const onOpenSettings = vi.fn()
    render(<Sidebar {...makeProps({ onOpenSettings })} />)
    await userEvent.click(screen.getByRole('button', { name: /open settings/i }))
    expect(onOpenSettings).toHaveBeenCalledTimes(1)
  })

  it('renders Inbox with the unsorted count', () => {
    render(<Sidebar {...makeProps({ inboxCount: 7 })} />)
    expect(screen.getByRole('button', { name: /inbox/i })).toBeInTheDocument()
    expect(screen.getByText('7')).toBeInTheDocument()
  })

  it('marks Inbox as active when selection is { type: inbox }', () => {
    render(<Sidebar {...makeProps({ selection: { type: 'inbox' } })} />)
    expect(screen.getByRole('button', { name: /inbox/i })).toHaveAttribute('aria-current', 'page')
  })

  it('calls onSelect({ type: inbox }) when Inbox is clicked', async () => {
    const onSelect = vi.fn()
    render(<Sidebar {...makeProps({ onSelect })} />)
    await userEvent.click(screen.getByRole('button', { name: /inbox/i }))
    expect(onSelect).toHaveBeenCalledWith({ type: 'inbox' })
  })

  it('renders with default inboxCount of 0 when prop is omitted', () => {
    render(<Sidebar {...makeProps()} />)
    const inboxBtn = screen.getByRole('button', { name: /inbox, 0 unsorted/i })
    expect(inboxBtn).toBeInTheDocument()
  })
})
