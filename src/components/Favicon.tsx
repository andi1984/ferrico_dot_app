import { useState } from 'react'
import { duckduckgoFavicon, initials } from '../utils'

// Stable, deterministic gradient picker from a string (domain or title).
// Hue rotates across the warm-iron palette so each site reads as its own tile,
// while staying on the Ferrico color story.
const GRADIENTS: Array<[string, string]> = [
  ['#3a2418', '#8a4a2a'], // rust
  ['#3a1d1d', '#a44a3a'], // red
  ['#3a2a14', '#b8893a'], // amber
  ['#15302e', '#3f8a82'], // teal
  ['#1a2540', '#5278b8'], // blue
  ['#241a3a', '#7a5fc0'], // violet
  ['#1a3038', '#4ea0b8'], // cyan
  ['#1d3022', '#5e9a6a'], // green
  ['#23272e', '#65707d'], // slate
]

function hash(s: string): number {
  let h = 0
  for (let i = 0; i < s.length; i++) {
    h = (h * 31 + s.charCodeAt(i)) | 0
  }
  return Math.abs(h)
}

export function Favicon({ storedUrl, bookmarkUrl, title, size = 36, radius = 8 }: {
  storedUrl: string | null
  bookmarkUrl: string
  title: string
  size?: number
  radius?: number
}) {
  const [failed, setFailed] = useState(false)
  const src = storedUrl || duckduckgoFavicon(bookmarkUrl)

  if (!src || failed) {
    const [dark, light] = GRADIENTS[hash(bookmarkUrl || title) % GRADIENTS.length]
    const glyph = initials(title).slice(0, 1) || '?'
    return (
      <div
        aria-hidden="true"
        className="shrink-0 flex items-center justify-center select-none"
        style={{
          width: size,
          height: size,
          borderRadius: radius,
          background: `linear-gradient(135deg, ${dark} 0%, ${light} 130%)`,
          border: '1px solid rgba(255,255,255,0.06)',
          boxShadow: 'inset 0 1px 0 rgba(255,255,255,0.10), 0 1px 2px rgba(0,0,0,0.2)',
          color: 'rgba(255,255,255,0.92)',
          fontFamily: 'var(--font-display)',
          fontSize: size * 0.42,
          fontWeight: 600,
          textShadow: '0 1px 0 rgba(0,0,0,0.2)',
        }}
      >{glyph}</div>
    )
  }

  return (
    <img
      src={src}
      alt=""
      draggable={false}
      className="shrink-0 object-contain"
      style={{
        width: size,
        height: size,
        borderRadius: radius,
        background: 'var(--bg-elev-strong)',
        border: '1px solid rgba(255,255,255,0.04)',
        padding: size > 28 ? 6 : 4,
      }}
      onError={() => setFailed(true)}
    />
  )
}
