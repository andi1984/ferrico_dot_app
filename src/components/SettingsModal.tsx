import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ModalShell, FieldLabel } from './ModalShell'
import { IconExport } from './icons'

export function SettingsModal({ onClose }: { onClose: () => void }) {
  const [token, setToken] = useState('')
  const [copied, setCopied] = useState(false)

  useEffect(() => { invoke<string>('get_api_token').then(setToken) }, [])

  function copy() {
    navigator.clipboard.writeText(token)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  async function handleExport() {
    const opml = await invoke<string>('export_opml')
    const blob = new Blob([opml], { type: 'text/xml' })
    const objectUrl = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = objectUrl
    a.download = 'ferrico-bookmarks.opml'
    a.click()
    URL.revokeObjectURL(objectUrl)
  }

  return (
    <ModalShell title="Settings" onClose={onClose}>
      <div className="p-6 flex flex-col gap-6">
        <div>
          <FieldLabel>Browser Extension Token</FieldLabel>
          <p className="text-xs mb-3" style={{ color: 'var(--text-secondary)' }}>
            Paste into the Ferrico extension options page to connect it.
          </p>
          <div className="flex gap-2">
            <code
              className="flex-1 px-3 py-2 rounded-lg text-xs font-mono truncate"
              style={{ background: 'var(--bg-elevated)', border: '1px solid var(--border-dim)', color: 'var(--text-secondary)' }}
            >
              {token || '…'}
            </code>
            <button
              onClick={copy}
              className="px-3 py-2 rounded-lg text-xs font-medium transition-all duration-150 cursor-pointer"
              style={{
                background: copied ? 'var(--accent-dim)' : 'var(--bg-elevated)',
                border: '1px solid var(--border-mid)',
                color: copied ? 'var(--accent)' : 'var(--text-secondary)',
              }}
              aria-live="polite"
            >
              {copied ? 'Copied!' : 'Copy'}
            </button>
          </div>
        </div>

        <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.5rem' }}>
          <FieldLabel>Export</FieldLabel>
          <p className="text-xs mb-3" style={{ color: 'var(--text-secondary)' }}>
            Download all bookmarks as an OPML file.
          </p>
          <button
            onClick={handleExport}
            className="flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-all duration-150 cursor-pointer"
            style={{
              background: 'var(--bg-elevated)',
              border: '1px solid var(--border-mid)',
              color: 'var(--text-secondary)',
            }}
            onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
            onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
            aria-label="Export OPML"
          >
            <IconExport size={14} />
            Export OPML
          </button>
        </div>
      </div>
    </ModalShell>
  )
}
