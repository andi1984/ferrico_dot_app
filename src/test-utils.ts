import type { Bookmark, Folder, Tag } from './types'

export function makeTag(overrides?: Partial<Tag>): Tag {
  return {
    id: 'tag-1',
    name: 'Test Tag',
    color: '#bf8b5e',
    created_at: 1700000000,
    ...overrides,
  }
}

export function makeFolder(overrides?: Partial<Folder>): Folder {
  return {
    id: 'folder-1',
    name: 'Test Folder',
    parent_id: null,
    created_at: 1700000000,
    ...overrides,
  }
}

export function makeBookmark(overrides?: Partial<Bookmark>): Bookmark {
  return {
    id: 'bm-1',
    url: 'https://example.com',
    title: 'Example',
    description: null,
    favicon_url: null,
    cover_url: null,
    feed_url: null,
    folder_id: null,
    tags: [],
    created_at: 1700000000,
    updated_at: 1700000000,
    deleted_at: null,
    is_broken: false,
    last_checked_at: null,
    ...overrides,
  }
}
