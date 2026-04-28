// Smallest possible end-to-end check against ONE backend (no manager).
// 1. Connect to backend's /control, ask for "list" — confirms boot session exists.
// 2. Open /qa/<bootSid> stream to listen for qa-detected events.
// 3. Open /s/<bootSid> stream and send a unique prompt.
// 4. Report whether the answer comes back via /qa/.
const TOKEN = process.env.LLM_CHAT_AUTH_TOKEN || 'devtoken123';
const PORT = process.env.LLM_CHAT_WS_PORT || '7878';
const URL = `ws://127.0.0.1:${PORT}`;
const auth = (p) => `${URL}${p}?token=${encodeURIComponent(TOKEN)}`;

const dec = new TextDecoder();
const once = (ws, evt) => new Promise((r) => ws.addEventListener(evt, r, { once: true }));
const readJson = (e) =>
  JSON.parse(typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data)));

async function ctrl(req) {
  const ws = new WebSocket(auth('/control'));
  await once(ws, 'open');
  await once(ws, 'message'); // hello
  ws.send(JSON.stringify(req));
  const ev = await once(ws, 'message');
  ws.close();
  return readJson(ev);
}

(async () => {
  console.log(`backend=${URL}  token=${TOKEN}`);

  // 1. List sessions
  const list = await ctrl({ cmd: 'list' });
  console.log('list →', list);
  if (!list.sessions || list.sessions.length === 0) {
    console.error('no sessions in backend (boot session expected) — abort');
    process.exit(1);
  }
  const sid = list.sessions[0];
  console.log('using session:', sid);

  // 2. Subscribe to /qa/
  const qa = new WebSocket(auth(`/qa/${sid}`));
  const qaEvents = [];
  qa.addEventListener('message', (e) => {
    try {
      const j = readJson(e);
      if (j && j.num !== undefined) {
        qaEvents.push(j);
        const ans = (j.answer || '').replace(/\s+/g, ' ').slice(0, 100);
        console.log(`[qa] num=${j.num} q="${j.question}" a="${ans}"`);
      } else {
        console.log('[qa-banner]', JSON.stringify(j).slice(0, 100));
      }
    } catch { console.log('[qa] non-JSON'); }
  });
  await once(qa, 'open');
  console.log('/qa/ subscribed');

  // 3. Subscribe to /s/ and send prompt
  const s = new WebSocket(auth(`/s/${sid}`));
  s.binaryType = 'arraybuffer';
  let sBytes = 0;
  let foundAnswer = false;
  s.addEventListener('message', (e) => {
    const data = typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data));
    sBytes += data.length;
    if (/SIMPLEST/.test(data)) foundAnswer = true;
  });
  await once(s, 'open');
  console.log('/s/ subscribed; waiting 6s for boot to settle...');
  await new Promise((r) => setTimeout(r, 6000));

  console.log('sending prompt...');
  s.send('reply with only the single word SIMPLEST\r');

  // wait up to 60s for an answer
  for (let i = 0; i < 60 && !foundAnswer && qaEvents.length === 0; i++) {
    await new Promise((r) => setTimeout(r, 1000));
  }
  await new Promise((r) => setTimeout(r, 2000));
  s.close();
  qa.close();

  console.log(`\n=== summary ===`);
  console.log(`/s/ got ${sBytes} bytes; answer-substring-found=${foundAnswer}`);
  console.log(`/qa/ got ${qaEvents.length} qa-detected event(s)`);
  qaEvents.forEach((e, i) => console.log(`  Q${i + 1}: q="${e.question}" a="${(e.answer || '').slice(0, 80)}"`));
  if (qaEvents.length === 0 && foundAnswer) {
    console.log('\nDIAG: claude answered (raw stream OK), but parser produced 0 qa events — JS/parser bug');
  } else if (foundAnswer && qaEvents.some((e) => /SIMPLEST/.test(e.answer || ''))) {
    console.log('\nPASS: end-to-end works');
    process.exit(0);
  } else {
    console.log('\nFAIL: answer not seen at all');
  }
  process.exit(1);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
