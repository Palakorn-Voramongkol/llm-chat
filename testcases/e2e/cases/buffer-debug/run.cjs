// Capture EVERY byte claude emits via /s/<sid>, then dump with the markers
// the parser looks for (`>` for question, `●` for answer) highlighted.
const fs = require('fs');
const os = require('os');
const path = require('path');

const TOKEN = fs.readFileSync(
  path.join(os.homedir(), '.local/share/com.llm-chat.app/auth.token'),
  'utf8'
).trim();
const MGR = 'ws://127.0.0.1:7777';
const auth = (p) => `${MGR}${p}?token=${encodeURIComponent(TOKEN)}`;

const dec = new TextDecoder();
const once = (ws, evt) => new Promise((r) => ws.addEventListener(evt, r, { once: true }));
const readJson = (e) =>
  JSON.parse(typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data)));

(async () => {
  const ctl = new WebSocket(auth('/control'));
  await once(ctl, 'open');
  await once(ctl, 'message');
  ctl.send(JSON.stringify({ cmd: 'open' }));
  let sid;
  while (!sid) {
    const ev = await once(ctl, 'message');
    const j = readJson(ev);
    if (j.sessionId) sid = j.sessionId;
  }
  console.log('opened', sid);

  const s = new WebSocket(auth(`/s/${sid}`));
  s.binaryType = 'arraybuffer';
  let raw = '';
  s.addEventListener('message', (e) => {
    const data = typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data));
    raw += data;
  });
  await once(s, 'open');
  console.log('waiting 10s for claude banner...');
  await new Promise((r) => setTimeout(r, 10000));
  console.log(`pre-prompt bytes: ${raw.length}`);

  console.log('sending prompt...');
  s.send('reply with only the single word ALPHA\r');
  await new Promise((r) => setTimeout(r, 60000));
  console.log(`post-prompt bytes: ${raw.length}`);
  s.close();
  ctl.send(JSON.stringify({ cmd: 'close', sessionId: sid }));
  await new Promise((r) => setTimeout(r, 500));
  ctl.close();

  // Strip ANSI to see plain text
  const stripped = raw
    .replace(/\x1b\][^\x07]*\x07/g, '')
    .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
    .replace(/\x1b[78()=>]?/g, '');

  // Save full and stripped for inspection
  fs.writeFileSync('/tmp/claude-raw.bin', raw, 'binary');
  fs.writeFileSync('/tmp/claude-stripped.txt', stripped);

  console.log('\n--- key markers in raw ---');
  const counts = {
    '> (gt)': (raw.match(/>/g) || []).length,
    '\\u276F (bold gt)': (raw.match(/❯/g) || []).length,
    '● (bullet U+25CF)': (raw.match(/●/g) || []).length,
    'ALPHA (target word)': (raw.match(/ALPHA/g) || []).length,
    'Welcome': (raw.match(/Welcome/g) || []).length,
    'tip': (raw.match(/tip/gi) || []).length,
  };
  for (const [k, v] of Object.entries(counts)) console.log(`  ${k}: ${v}`);

  console.log('\n--- last 1500 chars of stripped ---');
  console.log(stripped.slice(-1500));

  process.exit(0);
})().catch((e) => { console.error('FATAL:', e); process.exit(1); });
