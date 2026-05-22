import { useState, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import Papa from 'papaparse'
import { IconClose } from './icons'

const CHUNK_SIZE = 500

interface Mapping {
  url: string | null
  title: string | null
  description: string | null
  favicon_url: string | null
  feed_url: string | null
  folder_name: string | null
  tag_names: string | null
}

interface ParsedCsv {
  headers: string[]
  rows: string[][]
  /** Pre-computed column name → index, avoids O(n) indexOf on every row */
  colIndex: Map<string, number>
}

interface ImportResult {
  imported: number
  errors: string[]
}

type Step = 'select' | 'mapping' | 'importing' | 'done'

const FIELDS: { key: keyof Mapping; label: string; required: boolean; hint?: string }[] = [
  { key: 'url',         label: 'URL',         required: true  },
  { key: 'title',       label: 'Title',        required: true  },
  { key: 'description', label: 'Description',  required: false },
  { key: 'favicon_url', label: 'Favicon URL',  required: false },
  { key: 'feed_url',    label: 'Feed URL',     required: false },
  { key: 'folder_name', label: 'Folder',       required: false, hint: 'Created if it doesn\'t exist' },
  { key: 'tag_names',   label: 'Tags',         required: false, hint: 'Comma- or semicolon-separated' },
]

function buildColIndex(headers: string[]): Map<string, number> {
  const m = new Map<string, number>()
  headers.forEach((h, i) => m.set(h, i))
  return m
}

function getCell(row: string[], colIndex: Map<string, number>, col: string | null): string {
  if (!col) return ''
  const idx = colIndex.get(col)
  return idx !== undefined ? (row[idx] ?? '').trim() : ''
}

export interface ImportCsvModalProps {
  onClose: () => void
  onDone: () => void
}

export function ImportCsvModal({ onClose, onDone }: ImportCsvModalProps) {
  const [step, setStep] = useState<Step>('select')
  const [csv, setCsv] = useState<ParsedCsv | null>(null)
  const [mapping, setMapping] = useState<Mapping>({ url: null, title: null, description: null, favicon_url: null, feed_url: null, folder_name: null, tag_names: null })
  const [claudeLoading, setClaudeLoading] = useState(false)
  const [claudeError, setClaudeError] = useState<string | null>(null)
  const [result, setResult] = useState<ImportResult | null>(null)
  const [importError, setImportError] = useState<string | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  function parseCsvFile(file: File) {
    // Use papaparse's async File API — reads in chunks, doesn't block the thread
    Papa.parse<string[]>(file, {
      header: false,
      skipEmptyLines: true,
      complete(res) {
        if (!res.data.length) return
        const [headerRow, ...dataRows] = res.data
        const colIndex = buildColIndex(headerRow)
        const parsed: ParsedCsv = { headers: headerRow, rows: dataRows, colIndex }
        setCsv(parsed)
        setStep('mapping')
        setClaudeLoading(true)
        setClaudeError(null)

        invoke<Mapping>('suggest_csv_mapping', {
          headers: parsed.headers,
          sampleRows: parsed.rows.slice(0, 5),
        })
          .then((suggested) => setMapping(suggested))
          .catch((err) => {
            const msg = err instanceof Error ? err.message : String(err)
            setClaudeError(msg)
          })
          .finally(() => setClaudeLoading(false))
      },
    })
  }

  function handleFileInput(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0]
    if (file) parseCsvFile(file)
  }

  function handleDrop(e: React.DragEvent) {
    e.preventDefault()
    setDragOver(false)
    const file = e.dataTransfer.files[0]
    if (file) parseCsvFile(file)
  }

  async function handleImport() {
    if (!csv) return
    setStep('importing')
    setImportError(null)

    // Build inputs once with pre-computed index — O(rows × fields), no repeated indexOf
    const inputs = csv.rows
      .map((row) => ({
        url:         getCell(row, csv.colIndex, mapping.url),
        title:       getCell(row, csv.colIndex, mapping.title),
        description: getCell(row, csv.colIndex, mapping.description) || null,
        favicon_url: getCell(row, csv.colIndex, mapping.favicon_url) || null,
        feed_url:    getCell(row, csv.colIndex, mapping.feed_url)    || null,
        folder_name: getCell(row, csv.colIndex, mapping.folder_name) || null,
        tag_names:   getCell(row, csv.colIndex, mapping.tag_names)   || null,
      }))
      .filter((b) => b.url && b.title)

    const total = inputs.length
    let imported = 0
    const errors: string[] = []
    setProgress({ done: 0, total })

    // Send in chunks — gives progress feedback without one huge IPC payload
    for (let offset = 0; offset < inputs.length; offset += CHUNK_SIZE) {
      const chunk = inputs.slice(offset, offset + CHUNK_SIZE)
      try {
        const r = await invoke<ImportResult>('import_bookmarks', { inputs: chunk })
        imported += r.imported
        // Shift row numbers in error messages to reflect global position
        errors.push(...r.errors.map((e) => e.replace(/^Row (\d+)/, (_, n) => `Row ${offset + Number(n)}`)))
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err)
        setImportError(msg)
        setStep('mapping')
        setProgress(null)
        return
      }
      setProgress({ done: Math.min(offset + CHUNK_SIZE, total), total })
    }

    setResult({ imported, errors })
    setStep('done')
    setProgress(null)
    onDone()
  }

  // Computed with colIndex — O(rows) not O(rows × headers)
  const validRowCount = csv && mapping.url && mapping.title
    ? csv.rows.filter((row) => {
        const url   = getCell(row, csv.colIndex, mapping.url)
        const title = getCell(row, csv.colIndex, mapping.title)
        return url && title
      }).length
    : 0

  const canImport = mapping.url !== null && mapping.title !== null && !claudeLoading

  const stepTitle: Record<Step, string> = {
    select:    'Import CSV',
    mapping:   'Map Fields',
    importing: 'Importing…',
    done:      'Import Complete',
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      style={{ background: 'rgba(0,0,0,0.75)', backdropFilter: 'blur(4px)' }}
      onClick={(e) => { if (e.target === e.currentTarget && step !== 'importing') onClose() }}
    >
      <div
        className="anim-scale-in w-full rounded-xl shadow-2xl overflow-hidden"
        style={{ background: 'var(--bg-surface)', border: '1px solid var(--border-mid)', maxWidth: '38rem' }}
        role="dialog"
        aria-modal="true"
        aria-label={stepTitle[step]}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4" style={{ borderBottom: '1px solid var(--border-dim)' }}>
          <span className="text-sm font-semibold tracking-wide" style={{ color: 'var(--text-primary)' }}>
            {stepTitle[step]}
          </span>
          {step !== 'importing' && (
            <button
              onClick={onClose}
              className="rounded p-1 transition-colors duration-150 cursor-pointer"
              style={{ color: 'var(--text-muted)' }}
              onMouseEnter={(e) => (e.currentTarget.style.color = 'var(--text-primary)')}
              onMouseLeave={(e) => (e.currentTarget.style.color = 'var(--text-muted)')}
              aria-label="Close dialog"
            >
              <IconClose size={15} />
            </button>
          )}
        </div>

        {/* Step: select */}
        {step === 'select' && (
          <div className="p-6 flex flex-col gap-4">
            <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
              Upload a CSV file. Claude will automatically suggest how its columns map to bookmark fields.
            </p>
            <div
              onDragOver={(e) => { e.preventDefault(); setDragOver(true) }}
              onDragLeave={() => setDragOver(false)}
              onDrop={handleDrop}
              onClick={() => fileInputRef.current?.click()}
              className="flex flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed py-10 cursor-pointer transition-all duration-150"
              style={{
                borderColor: dragOver ? 'var(--accent)' : 'var(--border-mid)',
                background: dragOver ? 'rgba(200,160,90,0.06)' : 'transparent',
              }}
            >
              <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" style={{ color: 'var(--text-muted)' }}>
                <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                <polyline points="14 2 14 8 20 8" />
                <line x1="12" y1="18" x2="12" y2="12" />
                <line x1="9" y1="15" x2="15" y2="15" />
              </svg>
              <span className="text-sm font-medium" style={{ color: 'var(--text-secondary)' }}>
                Drop a CSV file here, or click to browse
              </span>
              <input ref={fileInputRef} type="file" accept=".csv,text/csv" className="hidden" onChange={handleFileInput} />
            </div>
          </div>
        )}

        {/* Step: mapping */}
        {step === 'mapping' && csv && (
          <div className="p-6 flex flex-col gap-5 overflow-y-auto" style={{ maxHeight: '70vh' }}>
            {claudeError && (
              <div className="rounded-lg px-4 py-3 text-xs" style={{ background: 'rgba(224,82,82,0.1)', color: '#e07070', border: '1px solid rgba(224,82,82,0.2)' }}>
                Claude mapping unavailable — please map fields manually.
              </div>
            )}

            {claudeLoading && (
              <div className="flex items-center gap-2 text-xs" style={{ color: 'var(--text-muted)' }}>
                <span className="inline-block w-3 h-3 rounded-full border-2 animate-spin" style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }} />
                Claude is suggesting field mappings…
              </div>
            )}

            {importError && (
              <div className="rounded-lg px-4 py-3 text-xs" style={{ background: 'rgba(224,82,82,0.1)', color: '#e07070', border: '1px solid rgba(224,82,82,0.2)' }}>
                {importError}
              </div>
            )}

            {/* Mapping table */}
            <div>
              <p className="text-xs uppercase tracking-widest font-medium mb-3" style={{ color: 'var(--text-muted)' }}>
                Field Mapping · {csv.rows.length.toLocaleString()} rows detected
              </p>
              <div className="flex flex-col gap-2">
                {FIELDS.map(({ key, label, required, hint }) => (
                  <div key={key} className="flex items-center gap-3">
                    <div className="w-36 flex-none">
                      <span
                        className="text-sm"
                        style={{ color: required ? 'var(--text-primary)' : 'var(--text-secondary)' }}
                      >
                        {label}
                        {required && <span style={{ color: 'var(--accent)' }}> *</span>}
                      </span>
                      {hint && (
                        <p className="text-xs mt-0.5" style={{ color: 'var(--text-muted)' }}>{hint}</p>
                      )}
                    </div>
                    <select
                      value={mapping[key] ?? ''}
                      onChange={(e) => setMapping((m) => ({ ...m, [key]: e.target.value || null }))}
                      className="ff flex-1 text-sm"
                      disabled={claudeLoading}
                    >
                      <option value="">— not mapped —</option>
                      {csv.headers.map((h) => (
                        <option key={h} value={h}>{h}</option>
                      ))}
                    </select>
                  </div>
                ))}
              </div>
            </div>

            {/* Preview */}
            {mapping.url && mapping.title && csv.rows.length > 0 && (
              <div>
                <p className="text-xs uppercase tracking-widest font-medium mb-3" style={{ color: 'var(--text-muted)' }}>
                  Preview (first 3 rows)
                </p>
                <div className="rounded-lg overflow-hidden text-xs" style={{ border: '1px solid var(--border-dim)' }}>
                  <table className="w-full border-collapse">
                    <thead>
                      <tr style={{ background: 'var(--bg-elevated)' }}>
                        <th className="text-left px-3 py-2 font-medium" style={{ color: 'var(--text-muted)' }}>URL</th>
                        <th className="text-left px-3 py-2 font-medium" style={{ color: 'var(--text-muted)' }}>Title</th>
                        {mapping.description && (
                          <th className="text-left px-3 py-2 font-medium hidden sm:table-cell" style={{ color: 'var(--text-muted)' }}>Desc</th>
                        )}
                      </tr>
                    </thead>
                    <tbody>
                      {csv.rows.slice(0, 3).map((row, i) => (
                        <tr key={i} style={{ borderTop: '1px solid var(--border-dim)' }}>
                          <td className="px-3 py-2 max-w-0" style={{ color: 'var(--text-primary)', width: '45%' }}>
                            <span className="block truncate">{getCell(row, csv.colIndex, mapping.url)}</span>
                          </td>
                          <td className="px-3 py-2 max-w-0" style={{ color: 'var(--text-secondary)', width: '35%' }}>
                            <span className="block truncate">{getCell(row, csv.colIndex, mapping.title)}</span>
                          </td>
                          {mapping.description && (
                            <td className="px-3 py-2 max-w-0 hidden sm:table-cell" style={{ color: 'var(--text-muted)', width: '20%' }}>
                              <span className="block truncate">{getCell(row, csv.colIndex, mapping.description)}</span>
                            </td>
                          )}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}

            {/* Actions */}
            <div className="flex gap-2 pt-1">
              <button
                type="button"
                onClick={() => { setCsv(null); setStep('select') }}
                className="flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-colors duration-150 cursor-pointer"
                style={{ border: '1px solid var(--border-mid)', color: 'var(--text-secondary)' }}
                onMouseEnter={(e) => (e.currentTarget.style.borderColor = 'var(--border-bright)')}
                onMouseLeave={(e) => (e.currentTarget.style.borderColor = 'var(--border-mid)')}
              >
                Back
              </button>
              <button
                type="button"
                onClick={handleImport}
                disabled={!canImport}
                className="flex-1 px-4 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 cursor-pointer"
                style={{ background: 'var(--accent)', color: '#0c0b0a', opacity: canImport ? 1 : 0.4 }}
              >
                Import {canImport ? `${validRowCount.toLocaleString()} bookmark${validRowCount === 1 ? '' : 's'}` : '…'}
              </button>
            </div>
          </div>
        )}

        {/* Step: importing */}
        {step === 'importing' && (
          <div className="p-10 flex flex-col items-center gap-5">
            <span
              className="inline-block w-8 h-8 rounded-full border-2 animate-spin flex-none"
              style={{ borderColor: 'var(--accent)', borderTopColor: 'transparent' }}
            />
            {progress ? (
              <>
                <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
                  {progress.done.toLocaleString()} / {progress.total.toLocaleString()} bookmarks…
                </p>
                <div className="w-full rounded-full overflow-hidden" style={{ height: '4px', background: 'var(--border-dim)' }}>
                  <div
                    className="h-full rounded-full transition-all duration-300"
                    style={{
                      width: `${Math.round((progress.done / progress.total) * 100)}%`,
                      background: 'var(--accent)',
                    }}
                  />
                </div>
              </>
            ) : (
              <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>Preparing…</p>
            )}
          </div>
        )}

        {/* Step: done */}
        {step === 'done' && result && (
          <div className="p-6 flex flex-col gap-5">
            <div className="flex flex-col gap-2">
              <p className="text-sm font-medium" style={{ color: 'var(--text-primary)' }}>
                {result.imported.toLocaleString()} bookmark{result.imported === 1 ? '' : 's'} imported successfully.
              </p>
              {result.errors.length > 0 && (
                <div className="rounded-lg px-4 py-3 text-xs" style={{ background: 'rgba(224,82,82,0.08)', color: '#e07070', border: '1px solid rgba(224,82,82,0.15)' }}>
                  <p className="font-medium mb-1">{result.errors.length.toLocaleString()} row{result.errors.length === 1 ? '' : 's'} skipped:</p>
                  <ul className="space-y-0.5 list-disc list-inside">
                    {result.errors.slice(0, 5).map((e, i) => <li key={i}>{e}</li>)}
                    {result.errors.length > 5 && <li>…and {(result.errors.length - 5).toLocaleString()} more</li>}
                  </ul>
                </div>
              )}
            </div>
            <button
              onClick={onClose}
              className="w-full px-4 py-2 rounded-lg text-sm font-semibold transition-opacity duration-150 hover:opacity-90 cursor-pointer"
              style={{ background: 'var(--accent)', color: '#0c0b0a' }}
            >
              Done
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
