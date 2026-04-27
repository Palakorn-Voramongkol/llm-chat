// Detached Claude Stream window. Listens for `qa-detected` events emitted by
// the main window's parser (via the broadcast_qa Tauri command) and renders.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const seen = new Map(); // num -> entry element

function escapeHtml(str) {
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function render(num, question, answer) {
  const panel = document.getElementById('qa-panel-content');
  if (!panel) return;
  const entryId = `qa-entry-${num}`;
  let entry = document.getElementById(entryId);
  if (!entry) {
    if (num > 1 && seen.size > 0) {
      const hr = document.createElement('hr');
      hr.className = 'qa-separator';
      panel.appendChild(hr);
    }
    entry = document.createElement('div');
    entry.className = 'qa-entry';
    entry.id = entryId;
    panel.appendChild(entry);
    seen.set(num, entry);
  }
  entry.innerHTML =
    `<div><span class="qa-label">Q${num}:</span> <span class="qa-question">${escapeHtml(question)}</span></div>` +
    `<div><span class="qa-label">A${num}:</span> <span class="qa-answer">${escapeHtml(answer)}</span></div>`;
  panel.scrollTop = panel.scrollHeight;
}

listen('qa-detected', (event) => {
  const { num, question, answer } = event.payload || {};
  if (typeof num === 'number') render(num, question || '', answer || '');
});


document.getElementById('btn-clear')?.addEventListener('click', () => {
  const panel = document.getElementById('qa-panel-content');
  if (panel) panel.innerHTML = '';
  seen.clear();
});
document.getElementById('btn-copy')?.addEventListener('click', async () => {
  const panel = document.getElementById('qa-panel-content');
  if (!panel) return;
  try {
    await navigator.clipboard.writeText(panel.innerText);
    const btn = document.getElementById('btn-copy');
    btn.textContent = '✓ Copied';
    setTimeout(() => { btn.textContent = '📋 Copy'; }, 1500);
  } catch {}
});
document.getElementById('btn-save')?.addEventListener('click', async () => {
  const panel = document.getElementById('qa-panel-content');
  if (!panel) return;
  try {
    await invoke('save_terminal_output', { content: panel.innerText });
    const btn = document.getElementById('btn-save');
    btn.textContent = '✓ Saved';
    setTimeout(() => { btn.textContent = '💾 Save'; }, 2000);
  } catch {}
});
