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

async function saveBookmark(token, { url, title, description, folder_id, tag_ids }) {
  const res = await apiFetch('/bookmarks', token, {
    method: 'POST',
    body: JSON.stringify({
      url,
      title,
      description: description || null,
      favicon_url: faviconUrl(url),
      feed_url: null,
      folder_id: folder_id || null,
      tag_ids: tag_ids.length ? tag_ids : null,
    }),
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

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

const BACK_ICON = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6"/></svg>`
const PLUS_ICON = `<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>`

function renderForm(tab, folders, tags, token) {
  let selectedFolderId = null
  let selectedTagIds = []
  let newTagColor = TAG_COLORS[4].hex // blue default

  // ── Folder section ────────────────────────────────────────

  const folderOptions = folders
    .map(f => `<option value="${escHtml(f.id)}">${escHtml(f.name)}</option>`)
    .join('')

  // ── Tag section ───────────────────────────────────────────

  const tagChips = tags
    .map(t => `
      <button class="tag-chip" data-tag-id="${escHtml(t.id)}" style="--tag-color:${escHtml(t.color)}">
        <span class="tag-dot"></span>${escHtml(t.name)}
      </button>
    `)
    .join('')

  const swatches = TAG_COLORS
    .map(c => `
      <button class="swatch${c.hex === newTagColor ? ' selected' : ''}"
              data-color="${escHtml(c.hex)}"
              style="background:${escHtml(c.hex)}"
              title="${escHtml(c.label)}"></button>
    `)
    .join('')

  app.innerHTML = `
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
        <div id="folder-select-wrap">
          <select id="folder-select">
            <option value="">No folder</option>
            ${folderOptions}
            <option value="__new__">New folder…</option>
          </select>
        </div>
        <div id="folder-create-panel" class="creation-panel" style="display:none">
          <div class="panel-input-row">
            <button class="btn-back" id="folder-back" title="Back">${BACK_ICON}</button>
            <input id="new-folder-name" type="text" placeholder="Folder name" />
            <button class="btn-create-sm" id="new-folder-save">Create</button>
          </div>
        </div>
      </div>

      <div class="field">
        <label>Tags</label>
        <div class="tags-row" id="tags-row">
          ${tagChips}
          <button class="btn-add-tag" id="add-tag-btn">${PLUS_ICON}Add tag</button>
        </div>
        <div id="new-tag-panel" class="creation-panel" style="display:none">
          <input id="new-tag-name" type="text" placeholder="Tag name" />
          <div class="swatch-grid">${swatches}</div>
          <div class="panel-actions">
            <button class="btn-ghost-sm" id="new-tag-cancel">Cancel</button>
            <button class="btn-filled-sm" id="new-tag-save">Create</button>
          </div>
        </div>
      </div>

      <div class="btn-row">
        <button class="btn-cancel" id="cancel">Cancel</button>
        <button class="btn-save" id="save">Save</button>
      </div>
      <div class="status" id="status"></div>
    </div>
  `

  // ── Folder interactions ────────────────────────────────────

  const folderSelect = document.getElementById('folder-select')
  const folderSelectWrap = document.getElementById('folder-select-wrap')
  const folderCreatePanel = document.getElementById('folder-create-panel')

  folderSelect.addEventListener('change', () => {
    if (folderSelect.value === '__new__') {
      folderSelectWrap.style.display = 'none'
      folderCreatePanel.style.display = 'flex'
      document.getElementById('new-folder-name').focus()
    } else {
      selectedFolderId = folderSelect.value || null
    }
  })

  document.getElementById('folder-back').addEventListener('click', () => {
    folderSelectWrap.style.display = 'block'
    folderCreatePanel.style.display = 'none'
    folderSelect.value = selectedFolderId || ''
    document.getElementById('new-folder-name').value = ''
  })

  document.getElementById('new-folder-save').addEventListener('click', async () => {
    const name = document.getElementById('new-folder-name').value.trim()
    if (!name) return
    try {
      const folder = await apiCreateFolder(token, name)
      folders.push(folder)
      const opt = document.createElement('option')
      opt.value = folder.id
      opt.textContent = folder.name
      folderSelect.insertBefore(opt, folderSelect.querySelector('[value="__new__"]'))
      selectedFolderId = folder.id
      folderSelect.value = folder.id
      folderSelectWrap.style.display = 'block'
      folderCreatePanel.style.display = 'none'
      document.getElementById('new-folder-name').value = ''
    } catch {
      // silent fail — user can retry
    }
  })

  // ── Tag chip interactions ──────────────────────────────────

  const tagsRow = document.getElementById('tags-row')

  tagsRow.addEventListener('click', (e) => {
    const chip = e.target.closest('.tag-chip')
    if (!chip) return
    const id = chip.dataset.tagId
    if (selectedTagIds.includes(id)) {
      selectedTagIds = selectedTagIds.filter(t => t !== id)
      chip.classList.remove('selected')
    } else {
      selectedTagIds.push(id)
      chip.classList.add('selected')
    }
  })

  document.getElementById('add-tag-btn').addEventListener('click', () => {
    document.getElementById('new-tag-panel').style.display = 'flex'
    document.getElementById('new-tag-name').focus()
  })

  document.getElementById('new-tag-cancel').addEventListener('click', () => {
    document.getElementById('new-tag-panel').style.display = 'none'
    document.getElementById('new-tag-name').value = ''
  })

  document.querySelectorAll('.swatch').forEach(swatch => {
    swatch.addEventListener('click', () => {
      document.querySelectorAll('.swatch').forEach(s => s.classList.remove('selected'))
      swatch.classList.add('selected')
      newTagColor = swatch.dataset.color
    })
  })

  document.getElementById('new-tag-save').addEventListener('click', async () => {
    const name = document.getElementById('new-tag-name').value.trim()
    if (!name) return
    try {
      const tag = await apiCreateTag(token, name, newTagColor)
      tags.push(tag)
      selectedTagIds.push(tag.id)
      const addBtn = document.getElementById('add-tag-btn')
      const chip = document.createElement('button')
      chip.className = 'tag-chip selected'
      chip.dataset.tagId = tag.id
      chip.style.setProperty('--tag-color', tag.color)
      chip.innerHTML = `<span class="tag-dot"></span>${escHtml(tag.name)}`
      tagsRow.insertBefore(chip, addBtn)
      document.getElementById('new-tag-name').value = ''
      document.getElementById('new-tag-panel').style.display = 'none'
    } catch {
      // silent fail — user can retry
    }
  })

  // ── Save / Cancel ──────────────────────────────────────────

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
      await saveBookmark(token, { url, title, description, folder_id: selectedFolderId, tag_ids: selectedTagIds })
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

  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      document.getElementById('save').click()
    }
  })
}

async function init() {
  const token = await getToken()
  if (!token) {
    renderNoToken()
    return
  }
  const tab = await getCurrentTab()
  const [folders, tags] = await Promise.all([fetchFolders(token), fetchTags(token)])
  renderForm(tab, folders, tags, token)
  setTimeout(() => {
    const titleInput = document.getElementById('title')
    if (titleInput) titleInput.select()
  }, 50)
}

init()
