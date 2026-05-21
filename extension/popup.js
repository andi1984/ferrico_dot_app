const API_BASE = 'http://127.0.0.1:59432'
const app = document.getElementById('app')

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

function detectFeed(tab) {
  // Try to detect RSS feed URL from page (injected content would be needed for
  // full detection; here we fall back to null and let the app handle it)
  return null
}

function faviconUrl(url) {
  try {
    const { hostname } = new URL(url)
    return `https://icons.duckduckgo.com/ip3/${hostname}.ico`
  } catch {
    return null
  }
}

async function saveBookmark({ url, title, description }) {
  const token = await getToken()
  const res = await fetch(`${API_BASE}/bookmarks`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${token}`,
    },
    body: JSON.stringify({
      url,
      title,
      description: description || null,
      favicon_url: faviconUrl(url),
      feed_url: null,
      folder_id: null,
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

function renderForm(tab) {
  app.innerHTML = `
    <div class="form">
      <div>
        <label>URL</label>
        <input id="url" type="url" value="${escHtml(tab.url || '')}" placeholder="https://example.com" />
      </div>
      <div>
        <label>Title</label>
        <input id="title" type="text" value="${escHtml(tab.title || '')}" placeholder="Page title" />
      </div>
      <div>
        <label>Note</label>
        <textarea id="description" placeholder="Optional note…"></textarea>
      </div>
      <div class="btn-row">
        <button class="btn-cancel" id="cancel">Cancel</button>
        <button class="btn-save" id="save">Save</button>
      </div>
      <div class="status" id="status"></div>
    </div>
  `

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
      await saveBookmark({ url, title, description })
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

  // Submit on Cmd/Ctrl+Enter
  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      document.getElementById('save').click()
    }
  })
}

function escHtml(str) {
  return str
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

async function init() {
  const token = await getToken()
  if (!token) {
    renderNoToken()
    return
  }
  const tab = await getCurrentTab()
  renderForm(tab)
  // Focus title field after URL is already filled
  setTimeout(() => {
    const titleInput = document.getElementById('title')
    if (titleInput) titleInput.select()
  }, 50)
}

init()
