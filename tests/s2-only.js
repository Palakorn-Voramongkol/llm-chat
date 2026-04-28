// Send a single message to a freshly-spawned (session 2) only, verify.
const decoder = new TextDecoder();
function strip(s) {
  return s
    .replace(/\x1b\][^\x07]*\x07/g, '')
    .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
    .replace(/[\x00-\x1f\x7f]/g, ' ')
    .replace(/ +/g, ' ');
}

function ctrlOpen() {
  return new Promise((res, rej) => {
    const ws = new WebSocket('ws://127.0.0.1:7878/control');
    ws.addEventListener('open', () => ws.send(JSON.stringify({ cmd: 'open' })));
    ws.addEventListener('message', (e) => {
      try {
        const j = JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data)));
        if (j.sessionId) { ws.close(); res(j.sessionId); }
      } catch {}
    });
    ws.addEventListener('error', rej);
    setTimeout(() => rej(new Error('control timeout')), 4000);
  });
}

(async () => {
  const id = await ctrlOpen();
  console.log('spawned:', id);
  await new Promise(r => setTimeout(r, 2000));

  const ws = new WebSocket(`ws://127.0.0.1:7878/s/${id}`);
  ws.binaryType = 'arraybuffer';
  let raw = '';
  ws.addEventListener('open', () => {
    console.log('connected; sending prompt');
    ws.send('reply with one word: AVOCADO\r');
  });
  ws.addEventListener('message', (e) => {
    raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
  });

  await new Promise(r => setTimeout(r, 25000));
  ws.close();

  const cleaned = strip(raw);
  console.log('=== cleaned tail (last 1500 chars) ===');
  console.log(cleaned.slice(-1500));
  console.log('---');
  console.log('contains AVOCADO:', cleaned.includes('AVOCADO'));
  console.log('total bytes:', raw.length);
  process.exit(0);
})().catch(e => { console.error('FATAL:', e); process.exit(1); });
