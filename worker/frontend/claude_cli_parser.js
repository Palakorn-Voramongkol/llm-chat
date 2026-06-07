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

    // Route a diagnostic into the Rust `tracing` sink via the frontend_log
    // Tauri command, so parser/PTY-scrape logs share the backend log stream
    // (filter with RUST_LOG=frontend=debug). Best-effort: never throws.
    flog(level, source, message, data) {
      try {
        invoke('frontend_log', {
          level,
          source,
          message,
          data: data === undefined ? null
            : (typeof data === 'string' ? data : JSON.stringify(data)),
        }).catch(() => {});
      } catch (_) { /* invoke unavailable — ignore */ }
    },

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
      // Spinner glyphs the Claude CLI cycles for its activity indicator.
      if (/^[?✱✽✻✶✢✷✺]/.test(line)) return true;
      // Mode-hint footer: "⏵⏵ bypass permissions on (shift+tab to cycle)" etc.
      if (/^⏵|\(shift\+tab to cycle\)/.test(line)) return true;
      // Model + effort indicator line: "◉ xhigh · /effort"
      if (/^[◉◯]/.test(line)) return true;
      // Same status line but rendered with the ● bullet (identical to a real
      // answer bullet) or wrapped without its leading glyph: "● high · /effort"
      // / "high · /effort". Must be filtered explicitly or it is captured as
      // the answer — and, scraped during warmup, pre-empts claude's real reply.
      if (/·\s*\/effort\b/.test(line)) return true;
      // Agentic tool-use chrome: the ⎿ tool-result connector and the
      // "… +N lines (ctrl+o to expand)" truncation hint are TUI furniture,
      // not part of the answer.
      if (/^⎿/.test(line)) return true;
      if (/\(ctrl\+o to expand\)/.test(line)) return true;
      if (/^…?\s*\+\s*\d+\s+lines?\b/.test(line)) return true;
      // Tool-call invocation lines, e.g. "Bash(curl …)", "Web Search(…)",
      // optionally still carrying a leading ●/⏺ bullet from the scrape.
      if (/^[●◉⏺]?\s*(Bash|Read|Edit|MultiEdit|Write|Glob|Grep|Task|WebSearch|Web Search|WebFetch|Web Fetch|NotebookEdit|TodoWrite|BashOutput)\(/.test(line)) {
        return true;
      }
      if (/^\s*Claude Code/.test(line)) return true;
      if (/Welcome back|Tips for getting|Recent activity|No recent activity|Run \/init|CLAUDE\.md/.test(line)) return true;
      if (/Opus|Claude Max|Organization/.test(line)) return true;
      if (/for shortcuts/.test(line)) return true;
      if (/esc to interrupt|Deliberating|Thinking|Loading|Misting/.test(line)) return true;
      return false;
    },

    // Reconstruct logical lines from scraped xterm visual rows. A row that
    // filled (near) the full terminal width is a soft-wrap of the previous
    // logical line → join with a space; a shorter row begins a NEW logical
    // line → join with a newline. Blank rows ({text:''}) are paragraph breaks.
    // Restores list/code/paragraph structure that plain space-joining flattened.
    joinAnswer(rows) {
      // Box-drawing / table rows (and code-fence rows) are never soft-wraps —
      // each is its own logical line, so a rendered table keeps its shape
      // instead of collapsing into one space-joined line.
      const isStructural = (t) => /[│├┼┤┌┐└┘┬┴╭╮╰╯─━┃║═╬╠╣╦╩]/.test(t) || /^```/.test(t);
      let out = '';
      let prevFull = false;
      let prevStruct = false;
      let afterBreak = true; // start of answer or just after a blank row
      for (const r of rows) {
        if (r.text === '') {
          if (out) { out += '\n'; afterBreak = true; prevFull = false; prevStruct = false; }
          continue;
        }
        const struct = isStructural(r.text);
        if (!out) out = r.text;
        else if (afterBreak) out += '\n' + r.text;
        else if (struct || prevStruct) out += '\n' + r.text; // table/code rows
        else if (prevFull) out += ' ' + r.text;
        else out += '\n' + r.text;
        prevFull = r.full;
        prevStruct = struct;
        afterBreak = false;
      }
      return out.replace(/\n{3,}/g, '\n\n').trim();
    },

    scan() {
      if (!this.enabled) return;
      const lines = this.getBufferLines();
      this.flog('trace', 'parser::buffer', 'scan: raw xterm buffer', {
        sid: this.sessionId || null,
        rows: lines.length,
        nonEmpty: lines.filter((l) => l.trim() !== ''),
      });

      // Rows at/above this width are soft-wraps; shorter rows = new logical line.
      const wrapWidth = Math.max(40, Math.floor((term.cols || 80) * 0.82));

      let currentQ = '';
      let currentALines = []; // [{text, full}]; {text:''} = blank/paragraph break
      const pairs = [];

      for (let i = 0; i < lines.length; i++) {
        const raw = lines[i]; // trailing-trimmed already
        const line = raw.trim();
        if (!line) {
          // Blank line inside an answer = paragraph break; otherwise ignore.
          if (currentQ && currentALines.length > 0) {
            currentALines.push({ text: '', full: false });
          }
          continue;
        }

        const qMatch = line.match(/^[>\u276F]\s+(.+)/);
        if (qMatch) {
          if (currentQ && currentALines.length > 0) {
            pairs.push({ q: currentQ, a: this.joinAnswer(currentALines) });
          }
          currentQ = qMatch[1].trim();
          currentALines = [];
          continue;
        }

        if (/^[>\u276F]\s*$/.test(line)) continue;

        const aMatch = line.match(/^●\s*(.*)/);
        if (aMatch && currentQ) {
          // The ● bullet is shared by real answers AND the "● high · /effort"
          // status line. isChrome() catches the status line (and other chrome
          // that happens to start with ●); skip it so it is never the answer.
          if (this.isChrome(line)) continue;
          currentALines.push({ text: aMatch[1].trim(), full: raw.length >= wrapWidth });
          continue;
        }

        if (currentALines.length > 0 && currentQ) {
          if (this.isChrome(line)) continue;
          // Diagnostic: this is where a non-chrome line becomes answer text.
          // If welcome/status chrome (e.g. "high · /effort") slips past
          // isChrome(), it shows up here — the leak point for that bug.
          this.flog('debug', 'parser::a-push', 'append non-chrome line to answer', { row: i, line });
          currentALines.push({ text: line, full: raw.length >= wrapWidth });
        }
      }
      if (currentQ && currentALines.length > 0) {
        pairs.push({ q: currentQ, a: this.joinAnswer(currentALines) });
      }

      this.flog('debug', 'parser::pairs', 'extracted Q&A pairs', {
        sid: this.sessionId || null,
        count: pairs.length,
        pairs: pairs.map((p) => ({ q: p.q.slice(0, 80), a: p.a.slice(0, 120) })),
      });

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
          this.flog('info', 'parser::broadcast', 'broadcasting Q&A pair to backend', {
            num: this.qaCount,
            isNew,
            q: pair.q.slice(0, 120),
            a: pair.a,
            aLen: pair.a.length,
          });
          // Broadcast to WebSocket clients. sessionId+isNew are required —
          // without sessionId, broadcast_qa can't fan out to /qa/<sid>.
          invoke('broadcast_qa', {
            num: this.qaCount,
            question: pair.q,
            answer: pair.a,
            sessionId: this.sessionId || null,
            isNew,
          }).catch(() => {});
          const statusEl = document.getElementById('qa-status');
          if (statusEl) statusEl.textContent = `QA: ${this.qaCount}`;
        }
      }
    },

    renderToPanel(num, question, answer, isNew) {
      // Allow per-session panels (multi-session UI sets panelContentId).
      // Falls back to the original single-panel id when unset.
      const panel = document.getElementById(this.panelContentId || 'qa-panel-content');
      if (!panel) return;
      // Namespace the entry id by panel so per-session panels don't clash on
      // `qa-entry-1` (each session's first pair has num=1).
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
          const newPath = await invoke('get_qa_log_path');
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

// Export for use by terminal.js
window.createCliParser = createCliParser;
