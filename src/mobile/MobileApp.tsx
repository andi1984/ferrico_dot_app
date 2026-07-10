// Placeholder mobile root. Replaced by the real shell in #64 (state, data
// loading, theme, events).
export function MobileApp() {
  return (
    <div
      className="flex h-full w-full items-center justify-center"
      style={{ background: 'var(--bg)', color: 'var(--text-1)' }}
    >
      <span className="text-sm" style={{ fontFamily: 'var(--font-display)' }}>
        Ferrico
      </span>
    </div>
  )
}
