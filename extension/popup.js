const API_BASE = 'http://127.0.0.1:59432'
const app = document.getElementById('app')

const TAG_COLORS = [
  { label: 'Tan',    hex: '#d4b896' },
  { label: 'Red',    hex: '#ef4444' },
  { label: 'Yellow', hex: '#eab308' },
  { label: 'Green',  hex: '#22c55e' },
  { label: 'Blue',   hex: '#3b82f6' },
  { label: 'Purple', hex: '#a855f7' },
  { label: 'Pink',   hex: '#ec4899' },
  { label: 'Teal',   hex: '#14b8a6' },
]

// ── Utils ──────────────────────────────────────────────────────────────────────

async function getToken() {
  return new Promise((resolve) => {
    chrome.storage.local.get(['ferrico_token'], (result) => {
      resolve(result.ferrico_token || null)
    })
  })
}

async function getCurrentTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true })
  return tab
}

function faviconUrl(url) {
  try {
    const { hostname } = new URL(url)
    return `https://icons.duckduckgo.com/ip3/${hostname}.ico`
  } catch {
    return null
  }
}

function escHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

// ── API ────────────────────────────────────────────────────────────────────────

async function apiFetch(path, token, options = {}) {
  return fetch(`${API_BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${token}`,
      ...(options.headers || {}),
    },
  })
}

async function fetchFolders(token) {
  try {
    const res = await apiFetch('/folders', token)
    return res.ok ? res.json() : []
  } catch {
    return []
  }
}

async function fetchTags(token) {
  try {
    const res = await apiFetch('/tags', token)
    return res.ok ? res.json() : []
  } catch {
    return []
  }
}

async function fetchRelatedTags(token, tagIds) {
  if (!tagIds || tagIds.length === 0) return []
  try {
    const qs = encodeURIComponent(tagIds.join(','))
    const res = await apiFetch(`/tags/related?ids=${qs}`, token)
    return res.ok ? res.json() : []
  } catch {
    return []
  }
}

async function apiCreateFolder(token, name) {
  const res = await apiFetch('/folders', token, {
    method: 'POST',
    body: JSON.stringify({ name, parent_id: null }),
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

async function apiCreateTag(token, name, color) {
  const res = await apiFetch('/tags', token, {
    method: 'POST',
    body: JSON.stringify({ name, color }),
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

async function saveBookmark(token, { url, title, description, folder_id, tag_ids, cover_url }) {
  const res = await apiFetch('/bookmarks', token, {
    method: 'POST',
    body: JSON.stringify({
      url,
      title,
      description: description || null,
      favicon_url: faviconUrl(url),
      cover_url: cover_url || null,
      feed_url: null,
      folder_id: folder_id || null,
      tag_ids: tag_ids.length ? tag_ids : null,
    }),
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

// ── Folder tree helpers ────────────────────────────────────────────────────────

function buildFolderFlat(folders) {
  const map = {}
  folders.forEach(f => { map[f.id] = { ...f, children: [] } })
  const roots = []
  folders.forEach(f => {
    if (f.parent_id && map[f.parent_id]) {
      map[f.parent_id].children.push(map[f.id])
    } else {
      roots.push(map[f.id])
    }
  })
  const result = []
  function walk(node, depth) {
    result.push({ ...node, depth })
    node.children.sort((a, b) => a.name.localeCompare(b.name))
    node.children.forEach(c => walk(c, depth + 1))
  }
  roots.sort((a, b) => a.name.localeCompare(b.name))
  roots.forEach(r => walk(r, 0))
  return result
}

function getFolderPath(folders, id) {
  const map = {}
  folders.forEach(f => (map[f.id] = f))
  const parts = []
  let cur = map[id]
  while (cur) {
    parts.unshift(cur.name)
    cur = cur.parent_id ? map[cur.parent_id] : null
  }
  return parts.join(' / ')
}

// ── SVG icons ─────────────────────────────────────────────────────────────────

const PLUS_SVG = `<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>`
const CARET_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M6 9l6 6 6-6"/></svg>`
const CHECK_SVG = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M5 12l5 5L20 7"/></svg>`
const FOLDER_SVG = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V7z"/></svg>`
const IMAGE_SVG = `<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>`

// ── Page capture + content scraping ─────────────────────────────────────────────

/** Capture a JPEG screenshot of the visible tab (activeTab grant). */
async function captureScreenshot() {
  try {
    return await chrome.tabs.captureVisibleTab({ format: 'jpeg', quality: 60 })
  } catch {
    return null // restricted page (chrome://, store, PDF viewer, …)
  }
}

/** Inject a one-shot scraper into the active tab to harvest text for tag ideas. */
async function scrapePageContent(tabId) {
  if (tabId == null) return null
  try {
    const [res] = await chrome.scripting.executeScript({
      target: { tabId },
      func: () => {
        const attr = (sel) => document.querySelector(sel)?.content || ''
        const m = (n) => attr(`meta[name="${n}"]`) || attr(`meta[property="${n}"]`)
        const headings = Array.from(document.querySelectorAll('h1, h2'))
          .map((h) => h.textContent.trim())
          .filter(Boolean)
          .slice(0, 12)
          .join(' . ')
        const bodyText = (document.body?.innerText || '')
          .replace(/\s+/g, ' ')
          .slice(0, 4000)
        return {
          keywords: m('keywords'),
          desc: m('description') || m('og:description'),
          ogTitle: m('og:title'),
          ogImage: m('og:image') || m('twitter:image'),
          headings,
          bodyText,
        }
      },
    })
    return res?.result || null
  } catch {
    return null // injection blocked on this page
  }
}

// ── Keyword analysis for tag suggestions ─────────────────────────────────────────

const STOPWORDS = new Set([
  // English
  'the', 'and', 'for', 'are', 'but', 'not', 'you', 'all', 'any', 'can', 'her', 'was',
  'one', 'our', 'out', 'has', 'have', 'had', 'his', 'how', 'its', 'who', 'why', 'what',
  'when', 'where', 'with', 'this', 'that', 'they', 'them', 'then', 'than', 'from', 'into',
  'your', 'will', 'would', 'about', 'there', 'their', 'which', 'been', 'more', 'most',
  'such', 'some', 'other', 'also', 'only', 'just', 'over', 'here', 'were', 'these', 'those',
  'page', 'home', 'menu', 'login', 'sign', 'search', 'click', 'read', 'using', 'use', 'get',
  // German
  'und', 'der', 'die', 'das', 'den', 'dem', 'des', 'ein', 'eine', 'einen', 'einem', 'einer',
  'ist', 'sind', 'war', 'mit', 'auf', 'für', 'aus', 'bei', 'aber', 'oder', 'auch', 'nicht',
  'sich', 'wird', 'werden', 'kann', 'mehr', 'sehr', 'wie', 'was', 'wer', 'wenn', 'dass',
  'hier', 'über', 'unter', 'durch', 'zum', 'zur', 'vom',
])

function tokenize(text) {
  if (!text) return []
  return String(text)
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter((t) => t.length >= 3 && t.length <= 24 && !/^\d+$/.test(t) && !STOPWORDS.has(t))
}

/**
 * Turn the tab + scraped content into a searchable text blob, a frequency map
 * of weighted single-word tokens, and a ranked list of keyword candidates
 * (meta-keyword phrases first, then the most prominent single words).
 */
function analyzeContent(tab, content) {
  content = content || {}
  let host = ''
  try {
    host = new URL(tab.url || '').hostname.replace(/^www\./, '')
  } catch {}
  const domainWord = host.split('.')[0] || ''

  const fields = [
    { text: content.keywords || '', w: 6 },
    { text: tab.title || content.ogTitle || '', w: 4 },
    { text: domainWord, w: 4 },
    { text: content.headings || '', w: 3 },
    { text: content.desc || '', w: 2 },
    { text: content.bodyText || '', w: 1 },
  ]

  const freq = {}
  for (const f of fields) {
    for (const tok of tokenize(f.text)) {
      freq[tok] = (freq[tok] || 0) + f.w
    }
  }

  const text = [
    content.keywords, tab.title, content.ogTitle,
    content.headings, content.desc, content.bodyText, host,
  ].filter(Boolean).join(' ').toLowerCase()

  // Short, human-authored phrases from the meta keywords tag (e.g. "machine learning").
  const phrases = (content.keywords || '')
    .split(',')
    .map((s) => s.trim().toLowerCase())
    .filter((s) => s.length >= 3 && s.length <= 30 && s.split(/\s+/).length <= 3 && s.includes(' '))

  const singles = Object.keys(freq).sort((a, b) => freq[b] - freq[a])

  const seen = new Set()
  const keywords = []
  for (const k of [...phrases, ...singles]) {
    if (!seen.has(k)) { seen.add(k); keywords.push(k) }
  }

  return { text, freq, keywords, domainWord }
}

/** Stable color pick for one-click "new tag" suggestions. */
function hashColor(str) {
  let h = 0
  for (let i = 0; i < str.length; i++) h = (h * 31 + str.charCodeAt(i)) >>> 0
  return TAG_COLORS[h % TAG_COLORS.length].hex
}

// ── No-token screen ────────────────────────────────────────────────────────────

function renderNoToken() {
  app.innerHTML = `
    <div class="no-token">
      <p>No API token configured. Open the extension options to connect Ferrico.</p>
      <a href="#" id="open-options">Open Options</a>
    </div>
  `
  document.getElementById('open-options').addEventListener('click', (e) => {
    e.preventDefault()
    chrome.runtime.openOptionsPage()
  })
}

// ── Tag Autocomplete ───────────────────────────────────────────────────────────

function mountTagCombobox(container, tags, token, selectedTagIds, analysis, getRelated) {
  let query = ''
  let open = false
  let activeIndex = -1
  let newTagColor = TAG_COLORS[4].hex
  let createPanelEl = null

  // Suggestion state
  let relatedCache = []
  let relatedKey = null
  let relatedLoading = false

  // ── DOM ──
  const pillInput = document.createElement('div')
  pillInput.className = 'pill-input'

  const inputEl = document.createElement('input')
  inputEl.type = 'text'
  inputEl.placeholder = 'Search or add tags…'
  inputEl.autocomplete = 'off'
  inputEl.spellcheck = false

  const dropdown = document.createElement('div')
  dropdown.className = 'ac-dropdown'

  const suggEl = document.createElement('div')
  suggEl.className = 'suggestions'

  pillInput.appendChild(inputEl)
  container.appendChild(pillInput)
  container.appendChild(dropdown)
  container.appendChild(suggEl)

  // ── Helpers ──

  function tagById(id) {
    return tags.find(t => t.id === id)
  }

  function filteredTags() {
    const q = query.toLowerCase().trim()
    return tags
      .filter(t => t.name.toLowerCase().includes(q))
      .sort((a, b) => a.name.localeCompare(b.name))
  }

  function hasExactMatch() {
    const q = query.toLowerCase().trim()
    return q && tags.some(t => t.name.toLowerCase() === q)
  }

  // ── Render chips ──

  function renderChips() {
    pillInput.querySelectorAll('.tag-pill').forEach(el => el.remove())
    selectedTagIds.forEach(id => {
      const tag = tagById(id)
      if (!tag) return
      const pill = document.createElement('span')
      pill.className = 'tag-pill'
      pill.style.setProperty('--tag-color', tag.color)
      pill.innerHTML =
        `<span class="tag-dot"></span>` +
        `<span class="pill-name">${escHtml(tag.name)}</span>` +
        `<span class="pill-remove" data-id="${escHtml(id)}" title="Remove">×</span>`
      pillInput.insertBefore(pill, inputEl)
    })
  }

  // ── Tag suggestions (content + context aware) ──

  function addTagById(id) {
    if (!selectedTagIds.includes(id)) selectedTagIds.push(id)
    renderChips()
    if (open) renderDropdown()
    refreshSuggestions()
  }

  async function createTagQuick(name) {
    const existing = tags.find((t) => t.name.toLowerCase() === name.toLowerCase())
    if (existing) { addTagById(existing.id); return }
    try {
      const tag = await apiCreateTag(token, name, hashColor(name))
      if (!tags.find((t) => t.id === tag.id)) tags.push(tag)
      addTagById(tag.id)
    } catch {
      // silent fail
    }
  }

  // Existing tags whose name appears in the page text.
  function existingContentMatches() {
    if (!analysis) return []
    return tags
      .filter((t) => {
        if (selectedTagIds.includes(t.id)) return false
        const n = t.name.toLowerCase().trim()
        return n.length >= 3 && analysis.text.includes(n)
      })
      .sort((a, b) => (b.bookmark_count || 0) - (a.bookmark_count || 0))
  }

  // Prominent keywords that aren't tags yet → proposed as brand-new tags.
  function newKeywordSuggestions() {
    if (!analysis) return []
    const existingNames = new Set(tags.map((t) => t.name.toLowerCase().trim()))
    const out = []
    for (const k of analysis.keywords) {
      if (existingNames.has(k)) continue
      const isPhrase = k.includes(' ')
      if (!isPhrase && (analysis.freq[k] || 0) < 4) continue // skip weak single words
      out.push(k)
      if (out.length >= 6) break
    }
    return out
  }

  function buildSuggestionChips() {
    const chips = []
    const usedIds = new Set(selectedTagIds)
    const usedNames = new Set(
      selectedTagIds.map((id) => (tagById(id)?.name || '').toLowerCase())
    )
    const CAP = 8

    // 1. Context — tags co-occurring with what's already selected.
    for (const t of relatedCache) {
      if (chips.length >= CAP) break
      if (usedIds.has(t.id)) continue
      if (!tagById(t.id)) tags.push(t) // ensure local lookup for chip render
      chips.push({ type: 'existing', tag: t })
      usedIds.add(t.id)
      usedNames.add(t.name.toLowerCase())
    }

    // 2. Content — existing tags that match the page.
    for (const t of existingContentMatches()) {
      if (chips.length >= CAP) break
      if (usedIds.has(t.id)) continue
      chips.push({ type: 'existing', tag: t })
      usedIds.add(t.id)
      usedNames.add(t.name.toLowerCase())
    }

    // 3. Content — fresh keywords as new-tag proposals.
    for (const k of newKeywordSuggestions()) {
      if (chips.length >= CAP) break
      if (usedNames.has(k)) continue
      chips.push({ type: 'new', name: k })
      usedNames.add(k)
    }

    return chips
  }

  function renderSuggestions() {
    const chips = buildSuggestionChips()
    suggEl.innerHTML = ''
    if (chips.length === 0 && !relatedLoading) {
      suggEl.classList.remove('has-items')
      return
    }
    suggEl.classList.add('has-items')

    const label = document.createElement('div')
    label.className = 'sugg-label'
    label.innerHTML = (relatedLoading ? `<span class="sugg-spinner"></span>` : '') + 'Suggested'
    suggEl.appendChild(label)

    chips.forEach((c) => {
      const chip = document.createElement('button')
      chip.type = 'button'
      chip.className = 'sugg-chip' + (c.type === 'new' ? ' is-new' : '')
      chip.addEventListener('mousedown', (e) => e.preventDefault()) // keep input focus
      if (c.type === 'existing') {
        chip.innerHTML =
          `<span class="sugg-dot" style="background:${escHtml(c.tag.color)}"></span>` +
          `<span class="sugg-name">${escHtml(c.tag.name)}</span>`
        chip.title = `Add tag “${c.tag.name}”`
        chip.addEventListener('click', () => addTagById(c.tag.id))
      } else {
        chip.innerHTML =
          `<span class="sugg-plus">${PLUS_SVG}</span>` +
          `<span class="sugg-name">${escHtml(c.name)}</span>`
        chip.title = `Create & add tag “${c.name}”`
        chip.addEventListener('click', () => createTagQuick(c.name))
      }
      suggEl.appendChild(chip)
    })
  }

  async function loadRelated() {
    const key = [...selectedTagIds].sort().join(',')
    if (key === relatedKey) return
    relatedKey = key
    if (selectedTagIds.length === 0 || !getRelated) {
      relatedCache = []
      renderSuggestions()
      return
    }
    relatedLoading = true
    renderSuggestions()
    let result = []
    try {
      result = await getRelated([...selectedTagIds])
    } catch {
      result = []
    }
    if (key !== relatedKey) return // selection moved on; ignore stale response
    relatedCache = Array.isArray(result) ? result : []
    relatedLoading = false
    renderSuggestions()
  }

  function refreshSuggestions() {
    renderSuggestions()
    loadRelated()
  }

  // ── Render dropdown ──

  function renderDropdown() {
    dropdown.innerHTML = ''
    activeIndex = Math.max(-1, Math.min(activeIndex, totalItems() - 1))

    const items = filteredTags()
    const showCreate = query.trim() && !hasExactMatch()

    if (items.length === 0 && !showCreate) {
      const empty = document.createElement('div')
      empty.className = 'ac-item ac-empty'
      empty.textContent = query.trim() ? 'No matching tags' : 'No tags yet'
      dropdown.appendChild(empty)
      return
    }

    items.forEach((tag, i) => {
      const isSelected = selectedTagIds.includes(tag.id)
      const isActive = i === activeIndex
      const item = document.createElement('div')
      item.className = 'ac-item' +
        (isActive ? ' active' : '') +
        (isSelected ? ' is-selected' : '')
      item.dataset.index = i

      const countHtml = tag.bookmark_count != null
        ? `<span class="item-count">${tag.bookmark_count}</span>`
        : ''

      const checkHtml = isSelected
        ? `<span class="item-check">${CHECK_SVG}</span>`
        : ''

      item.innerHTML =
        `<span class="item-dot" style="background:${escHtml(tag.color)}"></span>` +
        `<span class="item-name">${escHtml(tag.name)}</span>` +
        countHtml +
        checkHtml

      item.addEventListener('mousedown', e => { e.preventDefault(); toggleTag(tag.id) })
      dropdown.appendChild(item)
    })

    if (showCreate) {
      const div = document.createElement('div')
      div.className = 'ac-divider'
      dropdown.appendChild(div)

      const createIdx = items.length
      const createItem = document.createElement('div')
      createItem.className = 'ac-item create-item' + (createIdx === activeIndex ? ' active' : '')
      createItem.dataset.index = createIdx
      createItem.innerHTML =
        PLUS_SVG +
        `<span class="item-name">Create "<strong>${escHtml(query.trim())}</strong>"</span>`
      createItem.addEventListener('mousedown', e => {
        e.preventDefault()
        openCreatePanel(query.trim())
      })
      dropdown.appendChild(createItem)
    }
  }

  function totalItems() {
    const showCreate = query.trim() && !hasExactMatch()
    return filteredTags().length + (showCreate ? 1 : 0)
  }

  // ── Open / close ──

  function openDropdown() {
    if (open) return
    if (createPanelEl) return
    open = true
    dropdown.classList.add('open')
    renderDropdown()
  }

  function closeDropdown() {
    if (!open) return
    open = false
    dropdown.classList.remove('open')
    pillInput.classList.remove('focused')
  }

  // ── Toggle selection ──

  function toggleTag(id) {
    const idx = selectedTagIds.indexOf(id)
    if (idx >= 0) {
      selectedTagIds.splice(idx, 1)
    } else {
      selectedTagIds.push(id)
    }
    renderChips()
    if (open) renderDropdown()
    refreshSuggestions()
    inputEl.focus()
  }

  // ── Create tag panel ──

  function openCreatePanel(name) {
    closeDropdown()
    if (createPanelEl) createPanelEl.remove()

    const panel = document.createElement('div')
    panel.className = 'creation-panel'
    panel.style.animation = 'panel-in 0.14s ease'

    const swatches = TAG_COLORS
      .map(c =>
        `<button class="swatch${c.hex === newTagColor ? ' selected' : ''}" ` +
        `data-color="${escHtml(c.hex)}" style="background:${escHtml(c.hex)}" ` +
        `title="${escHtml(c.label)}"></button>`
      )
      .join('')

    panel.innerHTML =
      `<input id="ctp-name" type="text" value="${escHtml(name)}" placeholder="Tag name" />` +
      `<div class="swatch-grid">${swatches}</div>` +
      `<div class="panel-actions">` +
        `<button class="btn-ghost-sm" id="ctp-cancel">Cancel</button>` +
        `<button class="btn-filled-sm" id="ctp-save">Create tag</button>` +
      `</div>`

    container.appendChild(panel)
    createPanelEl = panel

    const nameInput = panel.querySelector('#ctp-name')
    nameInput.focus()
    nameInput.select()

    panel.querySelectorAll('.swatch').forEach(swatch => {
      swatch.addEventListener('click', () => {
        panel.querySelectorAll('.swatch').forEach(s => s.classList.remove('selected'))
        swatch.classList.add('selected')
        newTagColor = swatch.dataset.color
      })
    })

    panel.querySelector('#ctp-cancel').addEventListener('click', () => {
      panel.remove()
      createPanelEl = null
      query = ''
      inputEl.value = ''
      inputEl.focus()
    })

    const doCreate = async () => {
      const tagName = nameInput.value.trim()
      if (!tagName) return
      try {
        const tag = await apiCreateTag(token, tagName, newTagColor)
        tags.push(tag)
        selectedTagIds.push(tag.id)
        renderChips()
        refreshSuggestions()
        panel.remove()
        createPanelEl = null
        query = ''
        inputEl.value = ''
        inputEl.focus()
      } catch {
        // silent fail
      }
    }

    panel.querySelector('#ctp-save').addEventListener('click', doCreate)
    nameInput.addEventListener('keydown', e => {
      if (e.key === 'Enter') { e.preventDefault(); doCreate() }
      if (e.key === 'Escape') panel.querySelector('#ctp-cancel').click()
    })
  }

  // ── Events ──

  inputEl.addEventListener('focus', () => {
    pillInput.classList.add('focused')
    openDropdown()
  })

  inputEl.addEventListener('input', () => {
    query = inputEl.value
    activeIndex = -1
    if (!open) openDropdown()
    else renderDropdown()
  })

  inputEl.addEventListener('keydown', e => {
    const total = totalItems()

    if (e.key === 'ArrowDown') {
      e.preventDefault()
      activeIndex = Math.min(activeIndex + 1, total - 1)
      if (!open) openDropdown()
      else renderDropdown()
      scrollActive()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      activeIndex = Math.max(activeIndex - 1, -1)
      renderDropdown()
      scrollActive()
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const items = filteredTags()
      const showCreate = query.trim() && !hasExactMatch()
      if (activeIndex >= 0 && activeIndex < items.length) {
        toggleTag(items[activeIndex].id)
      } else if (showCreate && activeIndex === items.length) {
        openCreatePanel(query.trim())
      } else if (items.length > 0) {
        toggleTag(items[0].id)
      } else if (showCreate) {
        openCreatePanel(query.trim())
      }
    } else if (e.key === 'Escape') {
      closeDropdown()
    } else if (e.key === 'Backspace' && !query && selectedTagIds.length > 0) {
      selectedTagIds.splice(selectedTagIds.length - 1, 1)
      renderChips()
      if (open) renderDropdown()
      refreshSuggestions()
    }
  })

  inputEl.addEventListener('blur', () => {
    setTimeout(() => {
      if (!container.contains(document.activeElement)) {
        closeDropdown()
      }
    }, 150)
  })

  pillInput.addEventListener('mousedown', e => {
    if (e.target === pillInput) {
      e.preventDefault()
      inputEl.focus()
    }
  })

  pillInput.addEventListener('click', e => {
    const removeBtn = e.target.closest('.pill-remove')
    if (removeBtn) {
      const id = removeBtn.dataset.id
      const idx = selectedTagIds.indexOf(id)
      if (idx >= 0) {
        selectedTagIds.splice(idx, 1)
        renderChips()
        if (open) renderDropdown()
        refreshSuggestions()
      }
    }
  })

  function scrollActive() {
    const el = dropdown.querySelector('.ac-item.active')
    if (el) el.scrollIntoView({ block: 'nearest' })
  }

  renderChips()
  refreshSuggestions()
}

// ── Folder Picker ──────────────────────────────────────────────────────────────

function mountFolderPicker(container, folders, token, folderState) {
  let query = ''
  let open = false
  let activeIndex = -1
  let flatFolders = buildFolderFlat(folders)
  let showingCreate = false

  // ── DOM ──
  const display = document.createElement('div')
  display.className = 'folder-display'

  const folderIconEl = document.createElement('span')
  folderIconEl.className = 'folder-icon'
  folderIconEl.innerHTML = FOLDER_SVG

  const inputEl = document.createElement('input')
  inputEl.type = 'text'
  inputEl.placeholder = 'No folder'
  inputEl.autocomplete = 'off'
  inputEl.spellcheck = false

  const clearBtn = document.createElement('span')
  clearBtn.className = 'folder-clear'
  clearBtn.title = 'Clear folder'
  clearBtn.textContent = '×'
  clearBtn.style.display = 'none'

  const caretEl = document.createElement('span')
  caretEl.className = 'folder-caret'
  caretEl.innerHTML = CARET_SVG

  display.appendChild(folderIconEl)
  display.appendChild(inputEl)
  display.appendChild(clearBtn)
  display.appendChild(caretEl)

  const dropdown = document.createElement('div')
  dropdown.className = 'ac-dropdown'

  container.appendChild(display)
  container.appendChild(dropdown)

  // ── Helpers ──

  function selectedPath() {
    return folderState.folderId ? getFolderPath(folders, folderState.folderId) : ''
  }

  function filtered() {
    const q = query.toLowerCase().trim()
    if (!q) return flatFolders
    return flatFolders.filter(f => f.name.toLowerCase().includes(q) || getFolderPath(folders, f.id).toLowerCase().includes(q))
  }

  function updateClear() {
    clearBtn.style.display = folderState.folderId ? 'block' : 'none'
  }

  // ── Render dropdown ──

  function renderDropdown() {
    if (showingCreate) return
    dropdown.innerHTML = ''

    const items = filtered()
    // +1 for "No folder", +1 for "New folder…"
    const total = items.length + 2

    // "No folder" row
    const noneEl = document.createElement('div')
    noneEl.className = 'ac-item' +
      (activeIndex === 0 ? ' active' : '') +
      (!folderState.folderId ? ' is-selected' : '')
    noneEl.innerHTML =
      `<span class="item-name" style="color:#64748b;font-style:italic">No folder</span>` +
      (!folderState.folderId ? `<span class="item-check">${CHECK_SVG}</span>` : '')
    noneEl.addEventListener('mousedown', e => { e.preventDefault(); selectFolder(null) })
    dropdown.appendChild(noneEl)

    // Folder items
    items.forEach((f, i) => {
      const realIdx = i + 1
      const isActive = realIdx === activeIndex
      const isSelected = folderState.folderId === f.id
      const item = document.createElement('div')
      item.className = 'ac-item' +
        (isActive ? ' active' : '') +
        (isSelected ? ' is-selected' : '')
      item.dataset.index = realIdx

      const indent = f.depth * 14
      const depthEl = f.depth > 0
        ? `<span class="folder-depth-arrow" style="padding-left:${indent}px">↳</span>`
        : `<span style="padding-left:${indent}px"></span>`

      item.innerHTML =
        depthEl +
        `<span class="item-name">${escHtml(f.name)}</span>` +
        (isSelected ? `<span class="item-check">${CHECK_SVG}</span>` : '')

      item.addEventListener('mousedown', e => { e.preventDefault(); selectFolder(f.id) })
      dropdown.appendChild(item)
    })

    // Divider + "New folder…"
    const divEl = document.createElement('div')
    divEl.className = 'ac-divider'
    dropdown.appendChild(divEl)

    const newIdx = items.length + 1
    const newItem = document.createElement('div')
    newItem.className = 'ac-item create-item' + (newIdx === activeIndex ? ' active' : '')
    newItem.innerHTML = PLUS_SVG + `<span class="item-name">New folder…</span>`
    newItem.addEventListener('mousedown', e => { e.preventDefault(); openCreateFolder() })
    dropdown.appendChild(newItem)
  }

  // ── Select / clear ──

  function selectFolder(id) {
    folderState.folderId = id
    query = ''
    inputEl.value = selectedPath()
    inputEl.placeholder = 'No folder'
    updateClear()
    closeDropdown()
  }

  // ── Open / close ──

  function openDropdown() {
    if (open) return
    open = true
    showingCreate = false
    activeIndex = -1
    display.classList.add('focused')
    dropdown.classList.add('open')
    inputEl.placeholder = selectedPath() || 'Search folders…'
    inputEl.value = ''
    renderDropdown()
  }

  function closeDropdown() {
    if (!open) return
    open = false
    showingCreate = false
    dropdown.classList.remove('open')
    display.classList.remove('focused')
    query = ''
    inputEl.value = selectedPath()
    inputEl.placeholder = 'No folder'
  }

  // ── Create folder inline ──

  function openCreateFolder() {
    showingCreate = true
    dropdown.innerHTML = ''

    const row = document.createElement('div')
    row.className = 'dropdown-create-row'

    const nameInput = document.createElement('input')
    nameInput.type = 'text'
    nameInput.placeholder = 'Folder name'

    const saveBtn = document.createElement('button')
    saveBtn.className = 'btn-create-sm'
    saveBtn.textContent = 'Create'

    const cancelBtn = document.createElement('button')
    cancelBtn.className = 'btn-cancel-sm'
    cancelBtn.textContent = 'Cancel'

    row.appendChild(nameInput)
    row.appendChild(cancelBtn)
    row.appendChild(saveBtn)
    dropdown.appendChild(row)

    setTimeout(() => nameInput.focus(), 10)

    const doCreate = async () => {
      const name = nameInput.value.trim()
      if (!name) return
      try {
        const folder = await apiCreateFolder(token, name)
        folders.push(folder)
        flatFolders = buildFolderFlat(folders)
        selectFolder(folder.id)
        showingCreate = false
      } catch {
        // silent fail
      }
    }

    saveBtn.addEventListener('mousedown', e => e.preventDefault())
    saveBtn.addEventListener('click', doCreate)

    cancelBtn.addEventListener('mousedown', e => e.preventDefault())
    cancelBtn.addEventListener('click', () => {
      showingCreate = false
      renderDropdown()
      inputEl.focus()
    })

    nameInput.addEventListener('keydown', e => {
      if (e.key === 'Enter') { e.preventDefault(); doCreate() }
      if (e.key === 'Escape') cancelBtn.click()
    })

    nameInput.addEventListener('blur', () => {
      setTimeout(() => {
        if (!container.contains(document.activeElement)) closeDropdown()
      }, 150)
    })
  }

  // ── Events ──

  inputEl.addEventListener('focus', () => {
    openDropdown()
  })

  inputEl.addEventListener('input', () => {
    query = inputEl.value
    activeIndex = -1
    if (!open) openDropdown()
    else renderDropdown()
  })

  inputEl.addEventListener('keydown', e => {
    const items = filtered()
    const total = items.length + 2 // "No folder" + "New folder…"

    if (e.key === 'ArrowDown') {
      e.preventDefault()
      activeIndex = Math.min(activeIndex + 1, total - 1)
      renderDropdown()
      scrollActive()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      activeIndex = Math.max(activeIndex - 1, 0)
      renderDropdown()
      scrollActive()
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (activeIndex === 0) {
        selectFolder(null)
      } else if (activeIndex > 0 && activeIndex <= items.length) {
        selectFolder(items[activeIndex - 1].id)
      } else {
        openCreateFolder()
      }
    } else if (e.key === 'Escape') {
      closeDropdown()
    }
  })

  inputEl.addEventListener('blur', () => {
    setTimeout(() => {
      if (!container.contains(document.activeElement)) closeDropdown()
    }, 150)
  })

  display.addEventListener('mousedown', e => {
    if (e.target === display || e.target === caretEl || e.target === folderIconEl) {
      e.preventDefault()
      if (open) closeDropdown()
      else { inputEl.focus(); openDropdown() }
    }
  })

  clearBtn.addEventListener('mousedown', e => {
    e.preventDefault()
    selectFolder(null)
    inputEl.focus()
  })

  function scrollActive() {
    const el = dropdown.querySelector('.ac-item.active')
    if (el) el.scrollIntoView({ block: 'nearest' })
  }

  updateClear()
}

// ── Main form ──────────────────────────────────────────────────────────────────

function previewCardHtml(tab, screenshot) {
  const url = tab.url || ''
  let display = url
  try {
    const u = new URL(url)
    display = u.hostname.replace(/^www\./, '') + (u.pathname === '/' ? '' : u.pathname)
  } catch {}

  const fav = faviconUrl(url)
  const favHtml = fav
    ? `<img class="preview-favicon" src="${escHtml(fav)}" alt="" onerror="this.style.display='none'" />`
    : ''

  const shotHtml = screenshot
    ? `<img class="preview-shot" src="${escHtml(screenshot)}" alt="Page preview" />`
    : `<div class="preview-shot placeholder">${IMAGE_SVG}</div>`

  return `
    <div class="page-preview">
      ${shotHtml}
      <div class="preview-meta">
        <div class="preview-title-row">
          ${favHtml}
          <span class="preview-title">${escHtml(tab.title || 'Untitled')}</span>
        </div>
        <span class="preview-url">${escHtml(display)}</span>
      </div>
    </div>
  `
}

function renderForm(pageInfo, folders, tags, token) {
  const { tab, content, screenshot } = pageInfo
  const selectedTagIds = []
  const folderState = { folderId: null }
  const analysis = analyzeContent(tab, content)

  app.innerHTML = `
    ${previewCardHtml(tab, screenshot)}
    <div class="form">
      <div class="field">
        <label>URL</label>
        <input id="url" type="url" value="${escHtml(tab.url || '')}" placeholder="https://example.com" />
      </div>
      <div class="field">
        <label>Title</label>
        <input id="title" type="text" value="${escHtml(tab.title || '')}" placeholder="Page title" />
      </div>
      <div class="field">
        <label>Note</label>
        <textarea id="description" placeholder="Optional note…"></textarea>
      </div>
      <div class="field">
        <label>Folder</label>
        <div id="folder-combobox" class="combobox-wrap"></div>
      </div>
      <div class="field">
        <label>Tags</label>
        <div id="tag-combobox" class="combobox-wrap"></div>
      </div>
      <div class="btn-row">
        <button class="btn-cancel" id="cancel">Cancel</button>
        <button class="btn-save" id="save">Save</button>
      </div>
      <div class="status" id="status"></div>
    </div>
  `

  mountFolderPicker(
    document.getElementById('folder-combobox'),
    folders,
    token,
    folderState
  )

  mountTagCombobox(
    document.getElementById('tag-combobox'),
    tags,
    token,
    selectedTagIds,
    analysis,
    (ids) => fetchRelatedTags(token, ids)
  )

  document.getElementById('cancel').addEventListener('click', () => window.close())

  document.getElementById('save').addEventListener('click', async () => {
    const url = document.getElementById('url').value.trim()
    const title = document.getElementById('title').value.trim()
    const description = document.getElementById('description').value.trim()
    const statusEl = document.getElementById('status')
    const saveBtn = document.getElementById('save')

    if (!url || !title) {
      statusEl.textContent = 'URL and title are required.'
      statusEl.className = 'status error'
      return
    }

    saveBtn.disabled = true
    saveBtn.textContent = 'Saving…'
    statusEl.textContent = ''

    try {
      await saveBookmark(token, {
        url,
        title,
        description,
        folder_id: folderState.folderId,
        tag_ids: selectedTagIds,
        cover_url: content?.ogImage || null,
      })
      statusEl.textContent = 'Saved!'
      statusEl.className = 'status success'
      setTimeout(() => window.close(), 800)
    } catch (err) {
      let msg = 'Failed to save.'
      if (err.message.includes('401')) msg = 'Invalid token. Check extension options.'
      if (err.message.includes('Failed to fetch')) msg = 'Ferrico app not running.'
      statusEl.textContent = msg
      statusEl.className = 'status error'
      saveBtn.disabled = false
      saveBtn.textContent = 'Save'
    }
  })

  document.addEventListener('keydown', e => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      document.getElementById('save').click()
    }
  })
}

// ── Init ───────────────────────────────────────────────────────────────────────

async function init() {
  const token = await getToken()
  if (!token) {
    renderNoToken()
    return
  }
  const tab = await getCurrentTab()
  const [folders, tags, content, screenshot] = await Promise.all([
    fetchFolders(token),
    fetchTags(token),
    scrapePageContent(tab?.id),
    captureScreenshot(),
  ])
  renderForm({ tab, content, screenshot }, folders, tags, token)
  setTimeout(() => {
    const titleInput = document.getElementById('title')
    if (titleInput) titleInput.select()
  }, 50)
}

init()
