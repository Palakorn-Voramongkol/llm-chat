// Lists every session that currently exists, sends a unique magic word to
// each one, and verifies the response on each WS only contains its OWN word.
const decoder = new TextDecoder();
function strip(s) {
  return s
    .replace(/\x1b\][^\x07]*\x07/g, '')
    .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
    .replace(/[\x00-\x1f\x7f]/g, ' ')
    .replace(/ +/g, ' ');
}

const MAGIC = ['MARMALADE', 'BUTTERSCOTCH', 'AVOCADO', 'SNORKEL', 'TANGERINE', 'KEROSENE'];

function listSessions() {
  return new Promise((res, rej) => {
    const ws = new WebSocket('ws://127.0.0.1:7878/');
    ws.addEventListener('message', (e) => {
      try {
        const j = JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data)));
        ws.close();
        res(j);
      } catch (err) { rej(err); }
    });
    ws.addEventListener('error', rej);
    setTimeout(() => rej(new Error('list timeout')), 3000);
  });
}

function chat(label, sessionId, magic, ms = 22000) {
  return new Promise((resolve) => {
    const url = `ws://127.0.0.1:7878/s/${sessionId}`;
    const ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';
    let raw = '';
    ws.addEventListener('open', () => {
      ws.send(`reply with the single word ${magic}\r`);
    });
    ws.addEventListener('message', (e) => {
      raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
    });
    setTimeout(() => {
      ws.close();
      const cleaned = strip(raw);
      const otherMagics = MAGIC.filter(x => x !== magic);
      resolve({
        label,
        sessionId,
        magic,
        ownMagicSeen: cleaned.includes(magic),
        foreignMagicSeen: otherMagics.find(x => cleaned.includes(x)) || null,
        bytes: raw.length,
      });
    }, ms);
  });
}

(async () => {
  const sessions = await listSessions();
  console.log('discovered sessions:', sessions);
  if (!Array.isArray(sessions) || sessions.length === 0) {
    console.log('no sessions; exiting');
    process.exit(2);
  }

  const tasks = sessions.map((sid, i) =>
    chat(`session#${i + 1}`, sid, MAGIC[i % MAGIC.length])
  );
  const results = await Promise.all(tasks);

  console.log('\n=== RESULTS ===');
  let pass = true;
  for (const r of results) {
    const status = r.ownMagicSeen && !r.foreignMagicSeen ? 'OK' : 'FAIL';
    if (status === 'FAIL') pass = false;
    console.log(`[${status}] ${r.label} (${r.sessionId.slice(0, 24)}…) sent "${r.magic}"`);
    console.log(`        ownMagicSeen=${r.ownMagicSeen}  foreignMagicSeen=${r.foreignMagicSeen || 'no'}  bytes=${r.bytes}`);
  }
  console.log(pass
    ? '\nPASS: every session received its own distinct response with no cross-talk.'
    : '\nFAIL: at least one session is missing/wrong.');
  process.exit(pass ? 0 : 1);
})().catch(e => { console.error('FATAL:', e); process.exit(2); });
