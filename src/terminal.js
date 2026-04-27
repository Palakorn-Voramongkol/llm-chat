// Multi-session terminal logic. Each tab has its own xterm + claude PTY +
// claude_cli_parser. Switching tabs swaps which session's term + Q&A is shown.
const tauriInvoke = window.__TAURI__.core.invoke;
const { listen } = window.__TAURI__.event;
const utf8 = new TextDecoder('utf-8');

function decodeBase64UTF8(b64) {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return utf8.decode(bytes, { stream: true });
}

const sessions = new Map(); // id -> session
let activeId = null;
let nextSessionNum = 1;
const dbg = document.getElementById('debug-status');

function makeSession(id) {
  const num = nextSessionNum++;
  const label = `Session ${num}`;

  const termHost = document.createElement('div');
  termHost.className = 'term-host';
  termHost.id = `term-host-${id}`;
  document.getElementById('term-stack').appendChild(termHost);

  const qaContent = document.createElement('div');
  qaContent.className = 'qa-content';
  qaContent.id = `qa-content-${id}`;
  document.getElementById('qa-stack').appendChild(qaContent);

  // xterm.js doesn't initialize correctly inside a display:none host. Make the
  // new host visible (and the previously-active one hidden) BEFORE term.open
  // so xterm can measure real dimensions on first fit.
  for (const [, sess] of sessions) {
    sess.termHost.classList.remove('active');
    sess.qaContent.classList.remove('active');
  }
  termHost.classList.add('active');
  qaContent.classList.add('active');

  const term = new Terminal({
    cursorBlink: true,
    fontSize: 14,
    fontFamily: 'Consolas, "Courier New", monospace',
    theme: { background: '#ffffff', foreground: '#1a1a2e', cursor: '#3498db' },
    scrollback: 10000,
    minimumContrastRatio: 4.5,
  });
  const fitAddon = new FitAddon.FitAddon();
  term.loadAddon(fitAddon);
  term.open(termHost);
  fitAddon.fit();

  const parser = window.createClaudeCliParser(term, fitAddon);
  parser.panelContentId = `qa-content-${id}`;

  term.onData((data) => {
    tauriInvoke('pty_write', { sessionId: id, data }).catch((e) => {
      if (dbg) dbg.textContent = 'write err: ' + e;
    });
  });

  return { id, num, label, termHost, qaContent, term, fitAddon, parser };
}

function activeSession() {
  return activeId ? sessions.get(activeId) : null;
}

function activateSession(id) {
  for (const [sid, sess] of sessions) {
    const isActive = sid === id;
    sess.termHost.classList.toggle('active', isActive);
    sess.qaContent.classList.toggle('active', isActive);
  }
  activeId = id;
  // Tell Rust which session is currently visible so /control "current" can
  // report it to external clients.
  tauriInvoke('set_active_session', { sessionId: id }).catch(() => {});
  document.querySelectorAll('.tab').forEach((t) => {
    t.classList.toggle('active', t.dataset.id === id);
  });
  const sess = sessions.get(id);
  if (sess) {
    sess.term.focus();
    setTimeout(() => {
      sess.fitAddon.fit();
      tauriInvoke('pty_resize', {
        sessionId: id, cols: sess.term.cols, rows: sess.term.rows,
      }).catch(() => {});
    }, 50);
    const statusEl = document.getElementById('qa-status');
    if (statusEl) statusEl.textContent = `QA: ${sess.parser.qaCount || 0}`;
  }
}

async function addSession(externalId = null) {
  const id = externalId || `s${Date.now()}-${nextSessionNum}`;
  if (externalId && sessions.has(externalId)) return;
  const sess = makeSession(id);
  sessions.set(id, sess);

  // Single tab in the top tab bar; doubles as the QA-tab counter via a
  // small badge appended to the label whenever the parser detects pairs.
  const tab = document.createElement('div');
  tab.className = 'tab';
  tab.dataset.id = id;
  const labelSpan = document.createElement('span');
  labelSpan.className = 'tab-label';
  labelSpan.textContent = sess.label;
  const badge = document.createElement('span');
  badge.className = 'tab-badge';
  badge.textContent = '';
  const closeBtn = document.createElement('button');
  closeBtn.className = 'tab-close';
  closeBtn.title = 'Close session';
  closeBtn.textContent = '×';
  tab.appendChild(labelSpan);
  tab.appendChild(badge);
  tab.appendChild(closeBtn);
  tab.addEventListener('click', (e) => {
    if (e.target === closeBtn) {
      e.stopPropagation();
      closeSession(id);
    } else {
      activateSession(id);
    }
  });
  document.getElementById('btn-new-tab').before(tab);

  // Wrap renderToPanel so we update this session's count badge whenever a
  // pair is detected.
  const origRender = sess.parser.renderToPanel.bind(sess.parser);
  sess.parser.renderToPanel = function (num, q, a, isNew) {
    origRender(num, q, a, isNew);
    badge.textContent = sess.parser.qaCount > 0 ? ` (${sess.parser.qaCount})` : '';
  };

  activateSession(id);

  // If this UI was created in response to an external WS spawn, the Rust PTY
  // is already running for `id` — skip the spawn_session call.
  if (!externalId) {
    try {
      await tauriInvoke('spawn_session', {
        sessionId: id, cols: sess.term.cols, rows: sess.term.rows,
      });
    } catch (e) {
      if (dbg) dbg.textContent = 'spawn err: ' + e;
    }
  } else {
    // Resize the externally-spawned PTY to match this xterm
    tauriInvoke('pty_resize', {
      sessionId: id, cols: sess.term.cols, rows: sess.term.rows,
    }).catch(() => {});
    // Trigger parser start now (claude-session was emitted by Rust at spawn)
    sess.parser.sessionId = id;
    tauriInvoke('get_qa_log_path', { sessionId: id })
      .then((path) => sess.parser.start(path))
      .catch(() => sess.parser.start(''));
  }
}

const extAddedReady = listen('external-session-added', (event) => {
  const sid = event.payload?.sessionId;
  if (sid && !sessions.has(sid)) addSession(sid);
});

const extClosedReady = listen('external-session-closed', (event) => {
  const sid = event.payload?.sessionId;
  if (sid && sessions.has(sid)) closeSession(sid);
});

const extSwitchReady = listen('external-switch-session', (event) => {
  const sid = event.payload?.sessionId;
  if (sid && sessions.has(sid)) activateSession(sid);
});

const extClearStreamReady = listen('external-clear-stream', (event) => {
  const sid = event.payload?.sessionId;
  const sess = sessions.get(sid);
  if (!sess) return;
  sess.qaContent.innerHTML = '';
  sess.parser.savedQuestions = new Map();
  sess.parser.qaCount = 0;
  const tab = document.querySelector(`.tab[data-id="${sid}"] .tab-badge`);
  if (tab) tab.textContent = '';
});

const extClearTermReady = listen('external-clear-terminal', (event) => {
  const sid = event.payload?.sessionId;
  const sess = sessions.get(sid);
  if (sess) sess.term.clear();
});

function closeSession(id) {
  const sess = sessions.get(id);
  if (!sess) return;
  tauriInvoke('close_session', { sessionId: id }).catch(() => {});
  try { sess.term.dispose(); } catch {}
  sess.termHost.remove();
  sess.qaContent.remove();
  document.querySelector(`.tab[data-id="${id}"]`)?.remove();
  sessions.delete(id);
  if (activeId === id) {
    const next = sessions.keys().next().value;
    if (next) activateSession(next);
    else addSession();
  }
}

// PTY data → route to the session's term + parser
const ptyDataReady = listen('pty-data', (event) => {
  const payload = event.payload || {};
  const sess = sessions.get(payload.sessionId);
  if (!sess) return;
  const text = decodeBase64UTF8(payload.data);
  sess.term.write(text);
  sess.parser.onData();
});

// Per-session claude session start → tell the session's parser to begin
const claudeSessionReady = listen('claude-session', (event) => {
  const payload = event.payload || {};
  const sess = sessions.get(payload.sessionId);
  if (!sess) return;
  if (sess.parser.enabled) return;
  // Tag the parser with the session so its hourly rotation also lands in
  // a session-specific log file.
  sess.parser.sessionId = sess.id;
  tauriInvoke('get_qa_log_path', { sessionId: sess.id })
    .then((path) => sess.parser.start(path))
    .catch(() => sess.parser.start(''));
});

const ptyClosedReady = listen('pty-closed', (event) => {
  const sid = event.payload;
  const tab = document.querySelector(`.tab[data-id="${sid}"]`);
  if (tab) tab.classList.add('closed');
});

// New tab button
document.getElementById('btn-new-tab').addEventListener('click', () => addSession());

// Toolbar acts on active session
document.getElementById('btn-copy')?.addEventListener('click', async () => {
  const sess = activeSession(); if (!sess) return;
  const text = sess.term.getSelection() || '';
  if (text) {
    try {
      await navigator.clipboard.writeText(text);
      const btn = document.getElementById('btn-copy');
      btn.textContent = '✓ Copied';
      setTimeout(() => { btn.textContent = '📋 Copy'; }, 1500);
    } catch {}
  }
});

document.getElementById('btn-save')?.addEventListener('click', async () => {
  const sess = activeSession(); if (!sess) return;
  const lines = [];
  const buf = sess.term.buffer.active;
  for (let i = 0; i < buf.length; i++) {
    const line = buf.getLine(i);
    if (line) lines.push(line.translateToString(true));
  }
  try {
    await tauriInvoke('save_terminal_output', { content: lines.join('\n') });
    const btn = document.getElementById('btn-save');
    btn.textContent = '✓ Saved';
    setTimeout(() => { btn.textContent = '💾 Save'; }, 2000);
  } catch {}
});

document.getElementById('btn-clear')?.addEventListener('click', () => {
  const sess = activeSession(); if (!sess) return;
  sess.term.clear();
});

document.getElementById('btn-qa-clear')?.addEventListener('click', () => {
  const sess = activeSession(); if (!sess) return;
  sess.qaContent.innerHTML = '';
  sess.parser.savedQuestions = new Map();
  sess.parser.qaCount = 0;
  document.getElementById('qa-status').textContent = 'QA: 0';
});

document.getElementById('btn-qa-copy')?.addEventListener('click', async (e) => {
  e.stopPropagation();
  const sess = activeSession(); if (!sess) return;
  try {
    await navigator.clipboard.writeText(sess.qaContent.innerText);
    const btn = document.getElementById('btn-qa-copy');
    btn.textContent = '✓ Copied';
    setTimeout(() => { btn.textContent = '📋 Copy'; }, 1500);
  } catch {}
});

document.getElementById('btn-qa-save')?.addEventListener('click', async (e) => {
  e.stopPropagation();
  const sess = activeSession(); if (!sess) return;
  try {
    await tauriInvoke('save_terminal_output', { content: sess.qaContent.innerText });
    const btn = document.getElementById('btn-qa-save');
    btn.textContent = '✓ Saved';
    setTimeout(() => { btn.textContent = '💾 Save'; }, 2000);
  } catch {}
});

document.getElementById('btn-qa-log')?.addEventListener('click', async () => {
  const sess = activeSession(); if (!sess) return;
  if (sess.parser.enabled && sess.parser.logPath) {
    try { await tauriInvoke('open_qa_log', { path: sess.parser.logPath }); } catch {}
  } else {
    try {
      sess.parser.sessionId = sess.id;
      const path = await tauriInvoke('get_qa_log_path', { sessionId: sess.id });
      sess.parser.start(path);
    } catch {}
  }
});

// Window controls
document.querySelector('.terminal-topbar').addEventListener('mousedown', async (e) => {
  if (e.target.closest('.tb-btn')) return;
  try { await window.__TAURI__.window.getCurrentWindow().startDragging(); } catch {}
});
document.getElementById('btn-minimize')?.addEventListener('click', () => {
  try { window.__TAURI__.window.getCurrentWindow().minimize(); } catch {}
});
document.getElementById('btn-close')?.addEventListener('click', () => {
  try { window.__TAURI__.window.getCurrentWindow().close(); } catch {}
});

// Auto-resize the active session when the window/term-stack resizes
const resizeObserver = new ResizeObserver(() => {
  const sess = activeSession();
  if (!sess) return;
  sess.fitAddon.fit();
  tauriInvoke('pty_resize', {
    sessionId: sess.id, cols: sess.term.cols, rows: sess.term.rows,
  }).catch(() => {});
});
resizeObserver.observe(document.getElementById('term-stack'));

// Refocus the active xterm on window/document focus events
window.addEventListener('focus', () => activeSession()?.term.focus());
document.addEventListener('click', (e) => {
  if (e.target.closest('#qa-panel') || e.target.closest('.tool-btn') ||
      e.target.closest('.tab') || e.target.closest('#btn-new-tab') ||
      e.target.closest('#tab-bar')) return;
  activeSession()?.term.focus();
});

// ========== Layout switcher + resizer + detach (operate on shared chrome) ==========
(() => {
  const app = document.getElementById('terminal-app');
  const resizer = document.getElementById('qa-resizer');
  const panel = document.getElementById('qa-panel');
  const layoutBtn = document.getElementById('btn-layout');
  const header = panel?.querySelector('.qa-panel-header');
  const detachBtn = document.getElementById('btn-qa-detach');
  if (!app || !resizer || !panel || !layoutBtn || !header) return;

  const LAYOUTS = ['bottom', 'right', 'float'];
  const LABELS = { bottom: '⚯ Bottom', right: '⚯ Right', float: '⚯ Float' };
  let current = localStorage.getItem('claude-stream-layout') || 'bottom';
  if (!LAYOUTS.includes(current)) current = 'bottom';
  let lastDocked = current === 'float'
    ? (localStorage.getItem('claude-stream-last-docked') || 'bottom')
    : current;

  function refit() {
    const sess = activeSession();
    if (!sess) return;
    sess.fitAddon.fit();
    tauriInvoke('pty_resize', {
      sessionId: sess.id, cols: sess.term.cols, rows: sess.term.rows,
    }).catch(() => {});
  }

  function applyLayout(name) {
    LAYOUTS.forEach((l) => app.classList.remove('layout-' + l));
    app.classList.add('layout-' + name);
    layoutBtn.textContent = LABELS[name];
    if (detachBtn) detachBtn.innerHTML = name === 'float' ? '⎘ Attach' : '⎘ Detach';
    if (name !== 'float') {
      lastDocked = name;
      localStorage.setItem('claude-stream-last-docked', name);
    }
    panel.style.height = '';
    panel.style.width = '';
    panel.style.maxHeight = '';
    panel.style.top = '';
    panel.style.left = '';
    panel.style.right = '';
    panel.style.bottom = '';
    current = name;
    localStorage.setItem('claude-stream-layout', name);
    setTimeout(refit, 60);
  }
  applyLayout(current);
  layoutBtn.addEventListener('click', () => {
    const next = LAYOUTS[(LAYOUTS.indexOf(current) + 1) % LAYOUTS.length];
    applyLayout(next);
  });

  // ----- Resizer -----
  let dragging = false;
  let startX = 0, startY = 0, startH = 0, startW = 0;
  resizer.addEventListener('mousedown', (e) => {
    dragging = true;
    startX = e.clientX; startY = e.clientY;
    const r = panel.getBoundingClientRect();
    startH = r.height; startW = r.width;
    resizer.classList.add('dragging');
    document.body.style.userSelect = 'none';
    e.preventDefault(); e.stopPropagation();
  });
  window.addEventListener('mousemove', (e) => {
    if (!dragging) return;
    if (current === 'bottom') {
      const dy = startY - e.clientY;
      const newH = Math.max(80, Math.min(window.innerHeight - 120, startH + dy));
      panel.style.height = newH + 'px';
      panel.style.maxHeight = 'none';
    } else if (current === 'right') {
      const dx = startX - e.clientX;
      const newW = Math.max(200, Math.min(window.innerWidth - 200, startW + dx));
      panel.style.width = newW + 'px';
    }
  });
  window.addEventListener('mouseup', () => {
    if (!dragging) return;
    dragging = false;
    resizer.classList.remove('dragging');
    document.body.style.userSelect = '';
    refit();
  });

  // ----- Detach: real OS window -----
  let streamWin = null;
  async function openStreamWindow() {
    const ww = window.__TAURI__.webviewWindow;
    if (!ww) return;
    const existing = await ww.WebviewWindow.getByLabel('claude-stream');
    if (existing) {
      try { await existing.setFocus(); } catch {}
      return existing;
    }
    const win = new ww.WebviewWindow('claude-stream', {
      url: 'claude_stream.html',
      title: 'Claude Stream',
      width: 480, height: 600, decorations: true,
    });
    win.once('tauri://destroyed', () => {
      streamWin = null;
      app.classList.remove('app-detached');
      if (detachBtn) detachBtn.innerHTML = '⎘ Detach';
      setTimeout(refit, 60);
    });
    streamWin = win;
    return win;
  }
  async function closeStreamWindow() {
    if (!streamWin) return;
    try { await streamWin.close(); } catch {}
    streamWin = null;
  }
  if (detachBtn) {
    detachBtn.addEventListener('click', async (e) => {
      e.stopPropagation();
      if (streamWin) {
        await closeStreamWindow();
        app.classList.remove('app-detached');
        detachBtn.innerHTML = '⎘ Detach';
      } else {
        await openStreamWindow();
        app.classList.add('app-detached');
        detachBtn.innerHTML = '⎘ Attach';
      }
      setTimeout(refit, 60);
    });
  }

  // ----- Float drag from header -----
  let floatDrag = false;
  let fStartX = 0, fStartY = 0, fLeft = 0, fTop = 0;
  header.addEventListener('mousedown', (e) => {
    if (current !== 'float') return;
    if (e.target.closest('.tool-btn')) return;
    floatDrag = true;
    fStartX = e.clientX; fStartY = e.clientY;
    const r = panel.getBoundingClientRect();
    fLeft = r.left; fTop = r.top;
    panel.style.right = 'auto';
    panel.style.left = fLeft + 'px';
    panel.style.top = fTop + 'px';
    header.classList.add('dragging');
    document.body.style.userSelect = 'none';
    e.preventDefault(); e.stopPropagation();
  });
  window.addEventListener('mousemove', (e) => {
    if (!floatDrag) return;
    const dx = e.clientX - fStartX;
    const dy = e.clientY - fStartY;
    panel.style.left = Math.max(0, Math.min(window.innerWidth - 100, fLeft + dx)) + 'px';
    panel.style.top = Math.max(40, Math.min(window.innerHeight - 60, fTop + dy)) + 'px';
  });
  window.addEventListener('mouseup', () => {
    if (!floatDrag) return;
    floatDrag = false;
    header.classList.remove('dragging');
    document.body.style.userSelect = '';
  });
})();

// ========== Boot: await all listener registrations BEFORE signalling Rust
// that we're ready. Otherwise Rust may emit external-session-added (etc.)
// into the void when manager-driven sessions are created. ==========
if (dbg) dbg.textContent = 'JS loaded';
Promise.all([
  ptyDataReady,
  claudeSessionReady,
  ptyClosedReady,
  extAddedReady,
  extClosedReady,
  extSwitchReady,
  extClearStreamReady,
  extClearTermReady,
])
  .then(() => tauriInvoke('terminal_ready', { cols: 120, rows: 30 }))
  .then(() => {
    if (dbg) dbg.textContent = 'JS+Rust ready';
    addSession();
  })
  .catch((e) => {
    if (dbg) dbg.textContent = 'ERROR: ' + e;
  });
