// Drive enough control commands to push the per-day log over 1 MB and verify
// a new sequence file is created.
import fs from 'node:fs';
import path from 'node:path';
const decoder = new TextDecoder();
const once = (ws, evt) => new Promise((r) => ws.addEventListener(evt, r, { once: true }));
const readJson = (e) => JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data)));

(async () => {
  const ws = new WebSocket('ws://127.0.0.1:7878/control');
  await once(ws, 'open');
  await once(ws, 'message'); // hello

  // Find log directory via cmd:"log"
  ws.send(JSON.stringify({ cmd: 'log' }));
  const first = readJson(await once(ws, 'message'));
  const initialPath = first.path;
  const logDir = path.dirname(initialPath);
  console.log('initial log path:', initialPath);
  console.log('log dir:', logDir);

  // Snapshot existing log files
  const before = fs.readdirSync(logDir).filter((f) => f.startsWith('control_'));
  console.log('before:', before);

  // Hammer the control channel with many commands to grow the log
  // Each request+reply ~200 bytes; we want 1.5 MB to be sure rotation triggers
  const TARGET_REQS = 7500;
  console.log(`sending ${TARGET_REQS} list commands…`);
  let outstanding = 0;
  let sent = 0;
  let received = 0;
  await new Promise((resolve) => {
    function pump() {
      while (outstanding < 32 && sent < TARGET_REQS) {
        ws.send(JSON.stringify({ cmd: 'list' }));
        sent++;
        outstanding++;
      }
    }
    ws.addEventListener('message', () => {
      received++;
      outstanding--;
      if (received >= TARGET_REQS) resolve();
      else pump();
    });
    pump();
  });
  console.log(`sent ${sent}, received ${received}`);

  // Allow the writer to flush
  await new Promise((r) => setTimeout(r, 500));

  const after = fs.readdirSync(logDir).filter((f) => f.startsWith('control_'));
  console.log('after:', after);

  const today = new Date().toISOString().slice(0, 10).replace(/-/g, '');
  const todays = after.filter((f) => f.includes(`_${today}_`));
  todays.sort();
  console.log("today's files:", todays.map((f) => `${f} (${fs.statSync(path.join(logDir, f)).size} bytes)`));

  const seq1 = todays.find((f) => f.endsWith('_001.log'));
  const seq2 = todays.find((f) => f.endsWith('_002.log'));
  const seq1Size = seq1 ? fs.statSync(path.join(logDir, seq1)).size : 0;

  const pass =
    !!seq1 &&
    !!seq2 &&
    seq1Size <= 1024 * 1024 + 4096 && // soft cap; one line might overshoot
    seq1Size >= 1024 * 1024 - 50 * 1024;

  console.log(pass ? '\nPASS: rotation triggered correctly' : '\nFAIL');
  ws.close();
  process.exit(pass ? 0 : 1);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
