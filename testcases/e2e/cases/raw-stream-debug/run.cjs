// Debug: open one session via manager, listen on /s/<sid> raw stream,
// dump everything claude sends. Reveals whether the PTY and Tauri are working,
// independent of the webview/QA-parser layer.
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
  console.log('connecting /control...');
  const ctl = new WebSocket(auth('/control'));
  await once(ctl, 'open');
  await once(ctl, 'message');
  ctl.send(JSON.stringify({ cmd: 'open' }));
  let sid;
  while (!sid) {
    const ev = await once(ctl, 'message');
    const j = readJson(ev);
    if (j.sessionId) { sid = j.sessionId; console.log('opened', sid, 'on', j.backendPort); }
  }

  console.log('subscribing /s/...');
  const s = new WebSocket(auth(`/s/${sid}`));
  s.binaryType = 'arraybuffer';
  let total = 0;
  let chunks = 0;
  s.addEventListener('message', (e) => {
    const data = typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data));
    total += data.length;
    chunks++;
    if (chunks <= 3 || chunks % 20 === 0) {
      console.log(`[chunk ${chunks}, +${data.length}b, total=${total}b] ` +
        JSON.stringify(data.slice(0, 200)));
    }
  });
  await once(s, 'open');
  console.log('/s/ open. Waiting 6s for claude banner before sending...');
  await new Promise((r) => setTimeout(r, 6000));
  console.log('sending prompt...');
  s.send('what is 2+2? reply only the digit\r');
  await new Promise((r) => setTimeout(r, 30000));
  console.log(`\nFINAL: ${chunks} chunks, ${total} bytes received from claude`);
  s.close();
  ctl.send(JSON.stringify({ cmd: 'close', sessionId: sid }));
  await new Promise((r) => setTimeout(r, 500));
  ctl.close();
  process.exit(0);
})().catch((e) => { console.error('FATAL:', e); process.exit(1); });
