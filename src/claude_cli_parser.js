// ========== claude_cli_parser: Claude Code Q&A Stream Parser ==========
// Parses rendered xterm.js buffer lines to extract Q&A pairs from Claude CLI output.
// Source: claude_cli_parser.js

const { invoke } = window.__TAURI__.core;

function createClaudeCliParser(term, fitAddon) {
  return {
    logPath: '',
    qaCount: 0,
    enabled: false,
    debounceTimer: null,
    savedQuestions: new Map(),
    lastWrittenLen: new Map(),
    lastHour: 0,
    startTime: '',

    // Read rendered lines from the terminal buffer (clean, no escape codes)
    getBufferLines() {
      const lines = [];
      const buf = term.buffer.active;
      for (let i = 0; i < buf.length; i++) {
        const line = buf.getLine(i);
        if (line) lines.push(line.translateToString(true).trimEnd());
      }
      return lines;
    },

    // Check if a line is UI chrome (borders, status, etc.)
    isChrome(line) {
      if (!line) return true;
      if (/^[─│┌┐└┘├┤╭╮╰╯┬┴╔╗╚╝║═▶◀▲▼━┃╌╎\s]+$/.test(line)) return true;
      if (/^[?✱✽]/.test(line)) return true;
      if (/^\s*Claude Code/.test(line)) return true;
      if (/Welcome back|Tips for getting|Recent activity|No recent activity|Run \/init|CLAUDE\.md/.test(line)) return true;
      if (/Opus|Claude Max|Organization/.test(line)) return true;
      if (/for shortcuts/.test(line)) return true;
      if (/esc to interrupt|Deliberating|Thinking|Loading|Misting/.test(line)) return true;
      return false;
    },

    scan() {
      if (!this.enabled) return;
      const lines = this.getBufferLines();

      let currentQ = '';
      let currentALines = [];
      const pairs = [];

      for (let i = 0; i < lines.length; i++) {
        const line = lines[i].trim();
        if (!line) continue;

        const qMatch = line.match(/^[>❯]\s+(.+)/);
        if (qMatch) {
          if (currentQ && currentALines.length > 0) {
            pairs.push({ q: currentQ, a: currentALines.join('\n') });
          }
          currentQ = qMatch[1].trim();
          currentALines = [];
          continue;
        }

        if (/^[>❯]\s*$/.test(line)) continue;

        const aMatch = line.match(/^●\s*(.*)/);
        if (aMatch && currentQ) {
          currentALines.push(aMatch[1].trim());
          continue;
        }

        if (currentALines.length > 0 && currentQ) {
          if (this.isChrome(line)) continue;
          currentALines.push(line);
        }
      }
      if (currentQ && currentALines.length > 0) {
        pairs.push({ q: currentQ, a: currentALines.join('\n') });
      }

      for (const pair of pairs) {
        const qKey = pair.q.substring(0, 80);
        const prevAnswer = this.savedQuestions.get(qKey);
        if (prevAnswer === undefined || (pair.a.length > prevAnswer.length)) {
          const isNew = prevAnswer === undefined;
          this.savedQuestions.set(qKey, pair.a);
          if (isNew) {
            this.qaCount++;
          }
          // Write to file
          if (this.logPath) {
            if (isNew) {
              const entry = `\n--- Q${this.qaCount} ---\nQ: ${pair.q}\nA: ${pair.a}\n`;
              invoke('append_qa_log', { content: entry, path: this.logPath }).catch(() => {});
              this.lastWrittenLen.set(qKey, pair.a.length);
            } else {
              this.rewriteLog();
            }
          }
          // Update live panel
          this.renderToPanel(this.qaCount, pair.q, pair.a, isNew);
          // Broadcast to Tauri webviews + per-session WebSocket subscribers.
          invoke('broadcast_qa', {
            num: this.qaCount,
            question: pair.q,
            answer: pair.a,
            sessionId: this.sessionId || '',
            isNew,
          }).catch(() => {});
          const statusEl = document.getElementById('qa-status');
          if (statusEl) statusEl.textContent = `QA: ${this.qaCount}`;
        }
      }
    },

    renderToPanel(num, question, answer, isNew) {
      const panel = document.getElementById(this.panelContentId || 'qa-panel-content');
      if (!panel) return;
      // Namespace the entry id by panelContentId so that per-session panels
      // don't collide on `qa-entry-1` (each session's first Q&A has num=1).
      const ns = (this.panelContentId || 'qa-panel-content').replace(/^qa-content-/, '');
      const entryId = `qa-entry-${ns}-${num}`;
      let entry = document.getElementById(entryId);
      if (!entry) {
        if (num > 1) {
          const hr = document.createElement('hr');
          hr.className = 'qa-separator';
          panel.appendChild(hr);
        }
        entry = document.createElement('div');
        entry.className = 'qa-entry';
        entry.id = entryId;
        panel.appendChild(entry);
      }
      entry.innerHTML =
        `<div><span class="qa-label">Q${num}:</span> <span class="qa-question">${this.escapeHtml(question)}</span></div>` +
        `<div><span class="qa-label">A${num}:</span> <span class="qa-answer">${this.escapeHtml(answer)}</span></div>`;
      panel.scrollTop = panel.scrollHeight;
    },

    rewriteLog() {
      if (!this.logPath) return;
      let content = `=== Claude Code Q&A Log ===\nDate: ${this.startTime}\n`;
      let num = 0;
      for (const [qKey, answer] of this.savedQuestions) {
        num++;
        content += `\n--- Q${num} ---\nQ: ${qKey}\nA: ${answer}\n`;
      }
      invoke('write_qa_log', { content, path: this.logPath }).catch(() => {});
    },

    escapeHtml(str) {
      return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    },

    async checkRotation() {
      const now = new Date();
      const currentHour = now.getUTCFullYear() * 1000000 + (now.getUTCMonth()+1) * 10000 + now.getUTCDate() * 100 + now.getUTCHours();
      if (currentHour !== this.lastHour) {
        this.lastHour = currentHour;
        try {
          const newPath = await invoke('get_qa_log_path', { sessionId: this.sessionId });
          if (newPath !== this.logPath) {
            if (this.logPath) {
              invoke('append_qa_log', { content: `\n=== Continued in next file ===\n`, path: this.logPath }).catch(() => {});
            }
            this.logPath = newPath;
            const header = `=== Claude Code Q&A Log ===\nDate: ${now.toISOString()}\n=== Continued from previous file ===\n`;
            invoke('append_qa_log', { content: header, path: this.logPath }).catch(() => {});
          }
        } catch {}
      }
    },

    onData() {
      if (!this.enabled) return;
      if (this.debounceTimer) clearTimeout(this.debounceTimer);
      this.debounceTimer = setTimeout(() => {
        this.checkRotation();
        this.scan();
      }, 300);
    },

    start(path) {
      this.logPath = path;
      this.enabled = true;
      this.qaCount = 0;
      this.savedQuestions = new Map();
      this.lastWrittenLen = new Map();
      this.lastHour = 0;
      this.startTime = new Date().toISOString();
      if (this.logPath) {
        const header = `=== Claude Code Q&A Log ===\nDate: ${new Date().toISOString()}\n`;
        invoke('append_qa_log', { content: header, path: this.logPath }).catch(() => {});
      }
      const panel = document.getElementById('qa-panel');
      if (panel) panel.classList.add('visible');
      const content = document.getElementById(this.panelContentId || 'qa-panel-content');
      if (content) content.innerHTML = '';
      setTimeout(() => {
        if (fitAddon) {
          fitAddon.fit();
          invoke('pty_resize', { cols: term.cols, rows: term.rows }).catch(() => {});
        }
      }, 50);
    },

    stop() {
      this.scan(); // final scan
      this.enabled = false;
      if (this.debounceTimer) {
        clearTimeout(this.debounceTimer);
        this.debounceTimer = null;
      }
      const panel = document.getElementById('qa-panel');
      if (panel) panel.classList.remove('visible');
      setTimeout(() => {
        if (fitAddon) {
          fitAddon.fit();
          invoke('pty_resize', { cols: term.cols, rows: term.rows }).catch(() => {});
        }
      }, 50);
    }
  };
}

// Export for use by terminal.js
window.createClaudeCliParser = createClaudeCliParser;
