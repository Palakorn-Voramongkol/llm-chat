// Sends a unique magic word to each of two sessions and verifies that the
// distinct response comes back on the matching session's WS connection only.
const decoder = new TextDecoder();

function strip(s) {
  return s
    .replace(/\x1b\][^\x07]*\x07/g, '')          // OSC sequences
    .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')    // CSI sequences
    .replace(/[\x00-\x1f\x7f]/g, ' ')             // control chars
    .replace(/ +/g, ' ');
}

function chat(label, urlPath, magic, ms = 18000) {
  return new Promise((resolve) => {
    const url = `ws://127.0.0.1:7878${urlPath}`;
    const ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';
    let raw = '';
    ws.addEventListener('open', () => {
      ws.send(`reply with the single word ${magic}\r`);
    });
    ws.addEventListener('message', (e) => {
      const t = typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
      raw += t;
    });
    setTimeout(() => {
      ws.close();
      const cleaned = strip(raw);
      const otherMagics = ['ZEPHYR', 'LANTERN', 'PINEAPPLE', 'ELEPHANT', 'BUTTERSCOTCH', 'AVOCADO']
        .filter(x => x !== magic);
      resolve({
        label,
        urlPath,
        magic,
        mentionsExpected: cleaned.includes(magic),
        mentionsForeign: otherMagics.find(x => cleaned.includes(x)) || null,
        bytes: raw.length,
      });
    }, ms);
  });
}

function ctrlOpen() {
  return new Promise((res, rej) => {
    const ws = new WebSocket('ws://127.0.0.1:7878/control');
    let done = false;
    ws.addEventListener('open', () => ws.send(JSON.stringify({ cmd: 'open' })));
    ws.addEventListener('message', (e) => {
      try {
        const j = JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data)));
        if (j.sessionId) { done = true; ws.close(); res(j.sessionId); }
      } catch {}
    });
    ws.addEventListener('error', rej);
    setTimeout(() => { if (!done) rej(new Error('control timeout')); }, 4000);
  });
}

(async () => {
  // Use the existing session 1 + spawn a fresh one via control.
  const s2id = await ctrlOpen();
  console.log('spawned session 2:', s2id);
  await new Promise(r => setTimeout(r, 1500));

  const [r1, r2] = await Promise.all([
    chat('session1', '/s/1', 'BUTTERSCOTCH'),
    chat('session2', `/s/${s2id}`, 'AVOCADO'),
  ]);

  console.log('\n=== RESULTS ===');
  for (const r of [r1, r2]) {
    console.log(`${r.label} (${r.urlPath}) sent "${r.magic}":`);
    console.log(`  - response contains expected magic word: ${r.mentionsExpected}`);
    console.log(`  - response contains a foreign magic word: ${r.mentionsForeign || 'no'}`);
    console.log(`  - bytes received: ${r.bytes}`);
  }

  const pass = r1.mentionsExpected && r2.mentionsExpected
    && !r1.mentionsForeign && !r2.mentionsForeign;
  console.log(pass ? '\nPASS: each session got its own distinct response.' : '\nFAIL: cross-talk or missing response detected.');
  process.exit(pass ? 0 : 1);
})().catch(e => { console.error('FATAL:', e); process.exit(2); });
