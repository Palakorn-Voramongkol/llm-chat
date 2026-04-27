// Verify the manager delegates sessions across multiple backends.
// Connects to ws://127.0.0.1:7777/control, opens 4 sessions (manager runs 2
// backends, so it should round-robin), sends distinct prompts to each via
// the manager's /s/<sessionId>, and asserts each got its own response.
import fs from 'node:fs';
import path from 'node:path';
const decoder = new TextDecoder();

const TOKEN_PATH = path.join(process.env.TEMP || process.env.TMPDIR || '/tmp', 'llm-chat-qa', 'auth.token');
const TOKEN = fs.readFileSync(TOKEN_PATH, 'utf8').trim();
const Q = `?token=${encodeURIComponent(TOKEN)}`;
const strip = (s) => s
  .replace(/\x1b\][^\x07]*\x07/g, '')
  .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
  .replace(/[\x00-\x1f\x7f]/g, ' ')
  .replace(/ +/g, ' ');

function once(ws, evt) { return new Promise((r) => ws.addEventListener(evt, r, { once: true })); }
function readJson(e) { return JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data))); }

async function ctrl(ws, cmd) {
  ws.send(JSON.stringify(cmd));
  return readJson(await once(ws, 'message'));
}

function chat(label, sid, magic, ms = 22000) {
  return new Promise((res) => {
    const ws = new WebSocket(`ws://127.0.0.1:7777/s/${sid}${Q}`);
    ws.binaryType = 'arraybuffer';
    let raw = '';
    ws.addEventListener('open', () => ws.send(`reply with the single word ${magic}\r`));
    ws.addEventListener('message', (e) => {
      raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
    });
    setTimeout(() => {
      ws.close();
      const cleaned = strip(raw);
      res({ label, sid, magic, raw, cleaned, mentions: cleaned.includes(magic) });
    }, ms);
  });
}

(async () => {
  const w = new WebSocket(`ws://127.0.0.1:7777/control${Q}`);
  await once(w, 'open');
  await once(w, 'message');

  // Open 4 sessions through the manager
  const ids = [];
  for (let i = 0; i < 4; i++) {
    const r = await ctrl(w, { cmd: 'open' });
    if (!r.sessionId) throw new Error('open failed: ' + JSON.stringify(r));
    ids.push({ sid: r.sessionId, port: r.backendPort });
    console.log(`opened ${r.sessionId} on backend port ${r.backendPort}`);
  }

  // Verify the sessions are split across ≥2 distinct backend ports
  const ports = new Set(ids.map((i) => i.port));
  const distributed = ports.size >= 2;
  console.log('backend ports used:', [...ports]);

  // instances command
  const inst = await ctrl(w, { cmd: 'instances' });
  console.log('instances state:', inst);

  // wait long enough for every spawned claude.exe to reach its interactive
  // prompt. Cold start of a new Claude CLI is ~5–8s.
  console.log('warming up backends for 8s...');
  await new Promise((r) => setTimeout(r, 8000));

  // chat all 4 in parallel via the manager's /s, with a 30s budget so a
  // slow Claude has time to answer.
  const MAGIC = ['MAGNOLIA', 'TANGERINE', 'HEMLOCK', 'PISTACHIO'];
  const results = await Promise.all(
    ids.map((it, i) => chat(`s${i + 1}`, it.sid, MAGIC[i], 30000))
  );

  console.log('\n=== summary ===');
  let pass = distributed;
  for (const r of results) {
    const status = r.mentions ? 'OK' : 'FAIL';
    if (!r.mentions) pass = false;
    console.log(`[${status}] ${r.label} (${r.sid.slice(0, 22)}…) sent "${r.magic}"`);
  }

  // history through manager
  for (const it of ids) {
    const h = await ctrl(w, { cmd: 'history', sessionId: it.sid });
    const ok = (h.history || []).some((x) => MAGIC.some((m) => (x.answer || '').includes(m)));
    console.log(`  history ${it.sid.slice(0, 22)}… → ${(h.history || []).length} entries; matches a magic = ${ok}`);
    if (!ok) pass = false;
  }

  // Close all
  for (const it of ids) {
    const r = await ctrl(w, { cmd: 'close', sessionId: it.sid });
    if (!r.ok) pass = false;
  }
  const after = await ctrl(w, { cmd: 'list' });
  console.log('after close, list count:', after.count);
  // count should be back to whatever was before (each backend had 1 auto session = 2 total)
  pass = pass && after.count === 2;

  w.close();
  console.log(pass ? '\nPASS: manager delegates correctly across backends' : '\nFAIL');
  process.exit(pass ? 0 : 1);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
