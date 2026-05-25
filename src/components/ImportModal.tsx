import { useState, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ModalShell } from './ModalShell'
import { IconImport } from './icons'

interface ImportResult {
  imported: number
  errors: string[]
}

const IMPORT_COMMANDS: Record<string, string> = {
  json: 'import_json',
  html: 'import_netscape_html',
  htm:  'import_netscape_html',
  opml: 'import_opml',
  xml:  'import_opml',
}

const FORMAT_CHIPS = [
  { label: 'JSON',  ext: '.json' },
  { label: 'HTML',  ext: '.html' },
  { label: 'OPML',  ext: '.opml .xml' },
  { label: 'CSV',   ext: '.csv' },
]

export interface ImportModalProps {
  onClose: () => void
  onDone: () => void
  onImportCsv: () => void
}

type State =
  | { phase: 'idle' }
  | { phase: 'importing' }
  | { phase: 'done'; result: ImportResult }
  | { phase: 'error'; message: string }

export function ImportModal({ onClose, onDone, onImportCsv }: ImportModalProps) {
  const [state, setState] = useState<State>({ phase: 'idle' })
  const [dragOver, setDragOver] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  async function handleFile(file: File) {
    const ext = file.name.split('.').pop()?.toLowerCase() ?? ''

    if (ext === 'csv') {
      onClose()
      onImportCsv()
      return
    }

    const command = IMPORT_COMMANDS[ext]
    if (!command) {
      setState({ phase: 'error', message: `Unsupported file type: .${ext}. Use .json, .html, .opml, .xml, or .csv.` })
      return
    }

    setState({ phase: 'importing' })

    const text = await file.text()
    try {
      const result = await invoke<ImportResult>(
        command,
        ext === 'json' ? { json: text }
        : (ext === 'opml' || ext === 'xml') ? { xml: text }
        : { html: text },
      )
      setState({ phase: 'done', result })
      if (result.imported > 0) onDone()
    } catch (err) {
      const message =
        typeof err === 'object' && err !== null && 'message' in err
          ? (err as { message: string }).message
          : String(err)
      setState({ phase: 'error', message })
    }
  }

  function handleInputChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (file) handleFile(file)
    e.target.value = ''
  }

  function handleDrop(e: React.DragEvent) {
    e.preventDefault()
    setDragOver(false)
    const file = e.dataTransfer.files[0]
    if (file) handleFile(file)
  }

  const canClose = state.phase !== 'importing'

  return (
    <ModalShell title="Import Bookmarks" onClose={canClose ? onClose : () => {}}>
      <div className="p-6 flex flex-col gap-5">

        {/* Drop zone */}
        <div
          role="button"
          tabIndex={state.phase === 'importing' ? -1 : 0}
          aria-label="Choose file to import"
          aria-disabled={state.phase === 'importing'}
          onDragOver={(e) => { e.preventDefault(); if (state.phase !== 'importing') setDragOver(true) }}
          onDragLeave={() => setDragOver(false)}
          onDrop={state.phase !== 'importing' ? handleDrop : undefined}
          onClick={() => state.phase !== 'importing' && fileInputRef.current?.click()}
          onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && state.phase !== 'importing' && fileInputRef.current?.click()}
          className="flex flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed py-10 transition-all duration-150"
          style={{
            borderColor: dragOver ? 'var(--accent)' : state.phase === 'importing' ? 'var(--border-dim)' : 'var(--border-mid)',
            background: dragOver ? 'rgba(200,160,90,0.06)' : 'transparent',
            cursor: state.phase === 'importing' ? 'default' : 'pointer',
          }}
        >
          {state.phase === 'importing' ? (
            <>
              <span
                className="inline-block w-7 h-7 rounded-full border-2 animate-spin"
                style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
                aria-label="Importing…"
              />
              <span className="text-sm" style={{ color: 'var(--text-secondary)' }}>Importing…</span>
            </>
          ) : (
            <>
              <span style={{ color: 'var(--text-muted)' }}>
                <IconImport size={28} />
              </span>
              <div className="text-center">
                <p className="text-sm font-medium" style={{ color: 'var(--text-secondary)' }}>
                  Drop a file here, or click to browse
                </p>
              </div>
            </>
          )}
        </div>

        {/* Supported formats */}
        <div className="flex items-center gap-2 flex-wrap">
          <span className="text-xs" style={{ color: 'var(--text-muted)' }}>Supports</span>
          {FORMAT_CHIPS.map(({ label, ext }) => (
            <span
              key={label}
              className="px-2 py-0.5 rounded-md text-xs font-mono"
              style={{
                background: 'var(--bg-elevated)',
                border: '1px solid var(--border-dim)',
                color: 'var(--text-secondary)',
              }}
            >
              {label}
              <span style={{ color: 'var(--text-muted)' }}> {ext}</span>
            </span>
          ))}
          <span className="text-xs" style={{ color: 'var(--text-muted)' }}>· CSV opens field-mapping wizard</span>
        </div>

        {/* Result */}
        {state.phase === 'done' && (
          <div
            className="rounded-lg px-4 py-3 flex flex-col gap-2"
            style={{ background: 'rgba(100,200,120,0.07)', border: '1px solid rgba(100,200,120,0.2)' }}
          >
            <p className="text-sm font-medium" style={{ color: '#7dcf8e' }}>
              {state.result.imported.toLocaleString()} bookmark{state.result.imported === 1 ? '' : 's'} imported.
            </p>
            {state.result.errors.length > 0 && (
              <div className="text-xs" style={{ color: '#e07070' }}>
                <p className="font-medium mb-1">{state.result.errors.length} skipped:</p>
                <ul className="space-y-0.5 list-disc list-inside opacity-80">
                  {state.result.errors.slice(0, 5).map((e, i) => <li key={i}>{e}</li>)}
                  {state.result.errors.length > 5 && (
                    <li>…and {state.result.errors.length - 5} more</li>
                  )}
                </ul>
              </div>
            )}
          </div>
        )}

        {/* Error */}
        {state.phase === 'error' && (
          <div
            className="rounded-lg px-4 py-3 text-xs"
            style={{ background: 'rgba(224,82,82,0.1)', color: '#e07070', border: '1px solid rgba(224,82,82,0.2)' }}
          >
            {state.message}
          </div>
        )}

        {/* Footer button */}
        {(state.phase === 'done' || state.phase === 'error') && (
          <button
            onClick={state.phase === 'done' ? onClose : () => setState({ phase: 'idle' })}
            className="w-full px-4 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 hover:opacity-90 cursor-pointer"
            style={{ background: 'var(--accent)', color: '#0c0b0a' }}
          >
            {state.phase === 'done' ? 'Done' : 'Try Again'}
          </button>
        )}

        <input
          ref={fileInputRef}
          type="file"
          accept=".json,.html,.htm,.opml,.xml,.csv"
          className="hidden"
          onChange={handleInputChange}
          aria-hidden="true"
        />
      </div>
    </ModalShell>
  )
}
