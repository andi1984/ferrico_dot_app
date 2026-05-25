import { useState, useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ModalShell, FieldLabel } from './ModalShell'
import { IconExport, IconImport, IconLayers } from './icons'

interface ImportResult {
  imported: number
  errors: string[]
}

type ExportFormat = 'json' | 'html' | 'opml' | 'csv'

const EXPORT_FORMATS: { key: ExportFormat; label: string; command: string; ext: string; mime: string }[] = [
  { key: 'json',  label: 'JSON',          command: 'export_json',          ext: 'json', mime: 'application/json' },
  { key: 'html',  label: 'Netscape HTML', command: 'export_netscape_html', ext: 'html', mime: 'text/html' },
  { key: 'opml',  label: 'OPML',          command: 'export_opml',          ext: 'opml', mime: 'text/xml' },
  { key: 'csv',   label: 'CSV',           command: 'export_csv',           ext: 'csv',  mime: 'text/csv' },
]

const IMPORT_COMMANDS: Record<string, string> = {
  json: 'import_json',
  html: 'import_netscape_html',
  htm:  'import_netscape_html',
  opml: 'import_opml',
  xml:  'import_opml',
}

export interface SettingsModalProps {
  onClose: () => void
  onClear: () => void
  onDone: () => void
  onImportCsv: () => void
  onDeduplicate: () => void
}

export function SettingsModal({ onClose, onClear, onDone, onImportCsv, onDeduplicate }: SettingsModalProps) {
  const [token, setToken] = useState('')
  const [copied, setCopied] = useState(false)
  const [confirmClear, setConfirmClear] = useState(false)
  const [clearing, setClearing] = useState(false)
  const [exportBusy, setExportBusy] = useState<ExportFormat | null>(null)
  const [importResult, setImportResult] = useState<ImportResult | null>(null)
  const [importError, setImportError] = useState<string | null>(null)
  const [importing, setImporting] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  useEffect(() => { invoke<string>('get_api_token').then(setToken) }, [])

  function copy() {
    navigator.clipboard.writeText(token)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  async function handleExport(fmt: typeof EXPORT_FORMATS[number]) {
    setExportBusy(fmt.key)
    try {
      const content = await invoke<string>(fmt.command)
      const blob = new Blob([content], { type: fmt.mime })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `ferrico-bookmarks.${fmt.ext}`
      a.click()
      URL.revokeObjectURL(url)
    } finally {
      setExportBusy(null)
    }
  }

  function handleImportFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (!file) return
    e.target.value = ''

    const ext = file.name.split('.').pop()?.toLowerCase() ?? ''

    if (ext === 'csv') {
      onImportCsv()
      return
    }

    const command = IMPORT_COMMANDS[ext]
    if (!command) {
      setImportError(`Unsupported file type: .${ext}. Use .json, .html, .opml, .xml, or .csv.`)
      return
    }

    const reader = new FileReader()
    reader.onload = async (ev) => {
      const text = ev.target?.result as string
      setImporting(true)
      setImportError(null)
      setImportResult(null)
      try {
        const result = await invoke<ImportResult>(command, ext === 'json' ? { json: text } : ext === 'opml' || ext === 'xml' ? { xml: text } : { html: text })
        setImportResult(result)
        if (result.imported > 0) onDone()
      } catch (err) {
        const msg = typeof err === 'object' && err !== null && 'message' in err
          ? (err as { message: string }).message
          : String(err)
        setImportError(msg)
      } finally {
        setImporting(false)
      }
    }
    reader.readAsText(file)
  }

  async function handleClearConfirmed() {
    setClearing(true)
    try {
      await invoke('clear_all_data')
      onClear()
    } finally {
      setClearing(false)
    }
  }

  return (
    <ModalShell title="Settings" onClose={onClose}>
      <div className="p-6 flex flex-col gap-6">

        {/* Browser Extension Token */}
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

        {/* Import & Export */}
        <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.5rem' }}>
          <FieldLabel>Export</FieldLabel>
          <p className="text-xs mb-3" style={{ color: 'var(--text-secondary)' }}>
            Download all bookmarks. JSON is lossless (recommended for backup/sync); Netscape HTML works in all browsers.
          </p>
          <div className="flex flex-wrap gap-2">
            {EXPORT_FORMATS.map((fmt) => (
              <button
                key={fmt.key}
                onClick={() => handleExport(fmt)}
                disabled={exportBusy !== null}
                className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-all duration-150 cursor-pointer"
                style={{
                  background: 'var(--bg-elevated)',
                  border: '1px solid var(--border-mid)',
                  color: 'var(--text-secondary)',
                  opacity: exportBusy !== null && exportBusy !== fmt.key ? 0.5 : 1,
                }}
                onMouseEnter={(e) => { if (!exportBusy) e.currentTarget.style.borderColor = 'var(--border-bright)' }}
                onMouseLeave={(e) => { e.currentTarget.style.borderColor = 'var(--border-mid)' }}
              >
                {exportBusy === fmt.key
                  ? <span className="inline-block w-3 h-3 rounded-full border-2 animate-spin flex-none" style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }} />
                  : <IconExport size={13} />
                }
                {fmt.label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <FieldLabel>Import</FieldLabel>
          <p className="text-xs mb-3" style={{ color: 'var(--text-secondary)' }}>
            Supports JSON, Netscape HTML (.html), OPML (.opml/.xml), and CSV. CSV opens the field-mapping wizard.
          </p>

          <button
            onClick={() => fileInputRef.current?.click()}
            disabled={importing}
            className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-all duration-150 cursor-pointer"
            style={{
              background: 'var(--bg-elevated)',
              border: '1px solid var(--border-mid)',
              color: 'var(--text-secondary)',
            }}
            onMouseEnter={(e) => { if (!importing) e.currentTarget.style.borderColor = 'var(--border-bright)' }}
            onMouseLeave={(e) => { e.currentTarget.style.borderColor = 'var(--border-mid)' }}
          >
            {importing
              ? <span className="inline-block w-3 h-3 rounded-full border-2 animate-spin flex-none" style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }} />
              : <IconImport size={13} />
            }
            {importing ? 'Importing…' : 'Choose file to import…'}
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept=".json,.html,.htm,.opml,.xml,.csv"
            className="hidden"
            onChange={handleImportFile}
          />

          {importError && (
            <div className="mt-3 rounded-lg px-4 py-3 text-xs" style={{ background: 'rgba(224,82,82,0.08)', color: '#e07070', border: '1px solid rgba(224,82,82,0.15)' }}>
              {importError}
            </div>
          )}

          {importResult && (
            <div className="mt-3 rounded-lg px-4 py-3 text-xs flex flex-col gap-1" style={{ background: 'rgba(99,102,241,0.06)', border: '1px solid rgba(99,102,241,0.2)' }}>
              <p style={{ color: 'var(--text-primary)' }}>
                {importResult.imported.toLocaleString()} bookmark{importResult.imported === 1 ? '' : 's'} imported.
              </p>
              {importResult.errors.length > 0 && (
                <ul className="mt-1 space-y-0.5 list-disc list-inside" style={{ color: '#e07070' }}>
                  {importResult.errors.slice(0, 5).map((e, i) => <li key={i}>{e}</li>)}
                  {importResult.errors.length > 5 && <li>…and {importResult.errors.length - 5} more</li>}
                </ul>
              )}
            </div>
          )}
        </div>

        {/* Deduplication */}
        <div style={{ borderTop: '1px solid var(--border-dim)', paddingTop: '1.5rem' }}>
          <FieldLabel>Maintenance</FieldLabel>
          <p className="text-xs mb-3" style={{ color: 'var(--text-secondary)' }}>
            Find and remove bookmarks with the same URL. Tags from duplicates are merged into the one you keep.
          </p>
          <button
            onClick={() => { onDeduplicate(); onClose() }}
            className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium transition-all duration-150 cursor-pointer"
            style={{
              background: 'var(--bg-elevated)',
              border: '1px solid var(--border-mid)',
              color: 'var(--text-secondary)',
            }}
            onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
            onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
          >
            <IconLayers size={13} />
            Find duplicates…
          </button>
        </div>

        {/* Danger zone */}
        <div className="rounded-lg p-4 flex flex-col gap-3" style={{ border: '1px solid rgba(224,82,82,0.3)', background: 'rgba(224,82,82,0.04)' }}>
          <div>
            <p className="text-xs font-semibold uppercase tracking-widest mb-1" style={{ color: '#e07070' }}>Danger Zone</p>
            <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
              Permanently delete all bookmarks, folders, and tags. This cannot be undone.
            </p>
          </div>

          {!confirmClear ? (
            <button
              onClick={() => setConfirmClear(true)}
              className="self-start px-4 py-2 rounded-lg text-xs font-semibold transition-all duration-150 cursor-pointer"
              style={{ border: '1px solid rgba(224,82,82,0.4)', color: '#e07070', background: 'transparent' }}
              onMouseEnter={(e) => (e.currentTarget.style.background = 'rgba(224,82,82,0.1)')}
              onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
            >
              Clear all data…
            </button>
          ) : (
            <div className="flex flex-col gap-2">
              <p className="text-xs font-medium" style={{ color: '#e07070' }}>
                Are you sure? All data will be deleted immediately.
              </p>
              <div className="flex gap-2">
                <button
                  onClick={() => setConfirmClear(false)}
                  disabled={clearing}
                  className="flex-1 px-3 py-2 rounded-lg text-xs font-medium transition-colors duration-150 cursor-pointer"
                  style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
                  onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
                  onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
                >
                  Cancel
                </button>
                <button
                  onClick={handleClearConfirmed}
                  disabled={clearing}
                  className="flex-1 px-3 py-2 rounded-lg text-xs font-semibold transition-opacity duration-150 cursor-pointer"
                  style={{ background: '#c05050', color: '#fff', opacity: clearing ? 0.6 : 1 }}
                >
                  {clearing ? 'Deleting…' : 'Yes, delete everything'}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
    </ModalShell>
  )
}
