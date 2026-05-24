export interface Tag {
  id: string
  name: string
  color: string
  created_at: number
}

export interface Folder {
  id: string
  name: string
  parent_id: string | null
  created_at: number
}

export interface Bookmark {
  id: string
  url: string
  title: string
  description: string | null
  favicon_url: string | null
  feed_url: string | null
  folder_id: string | null
  tags: Tag[]
  created_at: number
  updated_at: number
  deleted_at: number | null
}

export type Selection =
  | { type: 'all' }
  | { type: 'inbox' }
  | { type: 'folder'; id: string }
  | { type: 'tag'; id: string }
  | { type: 'bin' }

export type ViewMode = 'list' | 'grid'

export type SortKey = 'date-desc' | 'date-asc' | 'title-asc' | 'title-desc' | 'domain-asc'
