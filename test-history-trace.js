// Trace test: open one session through the manager, send a prompt, read the
// response carefully, then query the OWNING backend for qa_history.
const decoder = new TextDecoder();
const once = (ws, evt) => new Promise((r) => ws.addEventListener(evt, r, { once: true }));

(async () => {
  // 1. Open via manager
  const m = new WebSocket('ws://127.0.0.1:7777/control');
  await once(m, 'open');
  await once(m, 'message'); // hello
  m.send(JSON.stringify({ cmd: 'open' }));
  const opened = JSON.parse((await once(m, 'message')).data);
  console.log('opened:', opened);
  const sid = opened.sessionId;
  const port = opened.backendPort;

  // 2. Wait for backend
  await new Promise((r) => setTimeout(r, 8000));

  // 3. Chat via manager
  const cs = new WebSocket(`ws://127.0.0.1:7777/s/${sid}`);
  cs.binaryType = 'arraybuffer';
  let raw = '';
  cs.addEventListener('message', (e) => {
    raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
  });
  await once(cs, 'open');
  cs.send('reply with the single word ZUCCHINI\r');
  await new Promise((r) => setTimeout(r, 25000));
  cs.close();
  const cleaned = raw
    .replace(/\x1b\][^\x07]*\x07/g, '')
    .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
    .replace(/[\x00-\x1f\x7f]/g, ' ')
    .replace(/ +/g, ' ');
  console.log('raw bytes:', raw.length, 'contains ZUCCHINI:', cleaned.includes('ZUCCHINI'));
  console.log('cleaned tail:', cleaned.slice(-400));

  // 4. Query backend directly for qa_history
  const b = new WebSocket(`ws://127.0.0.1:${port}/control`);
  await once(b, 'open');
  await once(b, 'message'); // hello
  b.send(JSON.stringify({ cmd: 'history', sessionId: sid }));
  const histResp = JSON.parse((await once(b, 'message')).data);
  console.log('backend history reply:', JSON.stringify(histResp).slice(0, 400));

  // 5. Also query the /qa/ stream
  const q = new WebSocket(`ws://127.0.0.1:${port}/qa/${sid}`);
  q.addEventListener('message', (e) => {
    console.log('/qa event:', typeof e.data === 'string' ? e.data : '(binary)');
  });
  await once(q, 'open');
  await new Promise((r) => setTimeout(r, 1500));
  q.close();

  m.close(); b.close();
  process.exit(0);
})().catch((e) => { console.error('FATAL:', e); process.exit(1); });
