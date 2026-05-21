const tokenInput = document.getElementById('token')
const saveBtn = document.getElementById('save')
const clearBtn = document.getElementById('clear')
const statusEl = document.getElementById('status')

// Load saved token on open
chrome.storage.local.get(['ferrico_token'], (result) => {
  if (result.ferrico_token) {
    tokenInput.value = result.ferrico_token
  }
})

saveBtn.addEventListener('click', async () => {
  const token = tokenInput.value.trim()
  if (!token) {
    showStatus('Token cannot be empty.', 'error')
    return
  }

  // Quick validation: ping the local server
  try {
    showStatus('Verifying…', '')
    // A POST with bad JSON returns 422, bad token returns 401, no server returns network error.
    // We just try to reach the server; 401 means server is up but token is wrong.
    const res = await fetch('http://127.0.0.1:59432/bookmarks', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`,
      },
      body: JSON.stringify({ url: '__ping__', title: '__ping__' }),
    })

    if (res.status === 401) {
      showStatus('Token rejected. Make sure you copied it correctly.', 'error')
      return
    }

    // 422 / 200 / other = server is reachable and token is accepted
    chrome.storage.local.set({ ferrico_token: token }, () => {
      showStatus('Token saved! Extension is connected.', 'success')
    })
  } catch {
    // Server not running — save anyway (will error at use time)
    chrome.storage.local.set({ ferrico_token: token }, () => {
      showStatus('Token saved. (Ferrico app is not running right now — that\'s OK.)', 'success')
    })
  }
})

clearBtn.addEventListener('click', () => {
  chrome.storage.local.remove(['ferrico_token'], () => {
    tokenInput.value = ''
    showStatus('Token cleared.', 'success')
  })
})

function showStatus(msg, type) {
  statusEl.textContent = msg
  statusEl.className = `status ${type}`
}
