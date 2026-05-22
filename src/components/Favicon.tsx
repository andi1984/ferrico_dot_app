import { useState } from 'react'
import { duckduckgoFavicon, initials } from '../utils'

export function Favicon({ storedUrl, bookmarkUrl, title }: {
  storedUrl: string | null
  bookmarkUrl: string
  title: string
}) {
  const [failed, setFailed] = useState(false)
  const src = storedUrl || duckduckgoFavicon(bookmarkUrl)

  if (!src || failed) {
    return (
      <div
        aria-hidden="true"
        className="w-7 h-7 rounded flex-none flex items-center justify-center text-xs font-semibold select-none"
        style={{ background: 'var(--accent-dim)', color: 'var(--accent)' }}
      >
        {initials(title)}
      </div>
    )
  }

  return (
    <img
      src={src}
      alt=""
      className="w-7 h-7 rounded object-contain flex-none"
      style={{ background: 'var(--bg-elevated)' }}
      onError={() => setFailed(true)}
    />
  )
}
