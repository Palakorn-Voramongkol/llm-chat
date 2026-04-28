// Verify the /qa/<id> WebSocket endpoint streams parsed Q&A pairs scoped to
// each session. Spawns 2 sessions, subscribes to both /qa channels, sends a
// distinct prompt to each, and checks that each /qa channel only emits its
// own Q&A — no cross-talk.
const decoder = new TextDecoder();

function once(ws, evt) {
  return new Promise((res) => ws.addEventListener(evt, res, { once: true }));
}
function readJson(e) {
  return JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data)));
}

async function ctrlOpen(ctl) {
  ctl.send(JSON.stringify({ cmd: 'open' }));
  while (true) {
    const ev = await once(ctl, 'message');
    const j = readJson(ev);
    if (j.sessionId) return j.sessionId;
  }
}

function subscribeQa(label, sid) {
  const ws = new WebSocket(`ws://127.0.0.1:7878/qa/${sid}`);
  const events = [];
  ws.addEventListener('message', (e) => {
    const text = typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
    try {
      const j = JSON.parse(text);
      events.push(j);
      console.log(`[qa:${label}]`, JSON.stringify(j).slice(0, 160));
    } catch {
      console.log(`[qa:${label}] (non-JSON)`, text.slice(0, 100));
    }
  });
  return { ws, events };
}

function chat(label, sid, magic, ms = 22000) {
  return new Promise((resolve) => {
    const ws = new WebSocket(`ws://127.0.0.1:7878/s/${sid}`);
    ws.binaryType = 'arraybuffer';
    ws.addEventListener('open', () => ws.send(`reply with single word ${magic}\r`));
    setTimeout(() => { ws.close(); resolve(); }, ms);
  });
}

(async () => {
  const ctl = new WebSocket('ws://127.0.0.1:7878/control');
  await once(ctl, 'open');
  await once(ctl, 'message');
  const a = await ctrlOpen(ctl);
  const b = await ctrlOpen(ctl);
  console.log('spawned A:', a);
  console.log('spawned B:', b);
  ctl.close();
  await new Promise(r => setTimeout(r, 1500));

  // subscribe BEFORE chatting so we don't miss any Q&A
  const subA = subscribeQa('A', a);
  const subB = subscribeQa('B', b);
  await new Promise(r => setTimeout(r, 800));

  // chat both in parallel
  await Promise.all([
    chat('A', a, 'BICYCLE'),
    chat('B', b, 'TRUMPET'),
  ]);

  await new Promise(r => setTimeout(r, 2000));
  subA.ws.close();
  subB.ws.close();

  // Filter only "qa-detected" payloads (skip the subscribed banner)
  const qaA = subA.events.filter((e) => e.num !== undefined);
  const qaB = subB.events.filter((e) => e.num !== undefined);

  console.log('\n=== summary ===');
  console.log(`/qa/${a} got ${qaA.length} Q&A event(s):`);
  qaA.forEach((e) => console.log(`   Q${e.num}: ${e.question} → A: ${(e.answer || '').slice(0, 80)}`));
  console.log(`/qa/${b} got ${qaB.length} Q&A event(s):`);
  qaB.forEach((e) => console.log(`   Q${e.num}: ${e.question} → A: ${(e.answer || '').slice(0, 80)}`));

  const aHasOwn = qaA.some((e) => (e.answer || '').includes('BICYCLE'));
  const aHasForeign = qaA.some((e) => (e.answer || '').includes('TRUMPET'));
  const bHasOwn = qaB.some((e) => (e.answer || '').includes('TRUMPET'));
  const bHasForeign = qaB.some((e) => (e.answer || '').includes('BICYCLE'));

  console.log('A own:', aHasOwn, 'A foreign:', aHasForeign);
  console.log('B own:', bHasOwn, 'B foreign:', bHasForeign);

  const pass = aHasOwn && bHasOwn && !aHasForeign && !bHasForeign;
  console.log(pass ? '\nPASS: each /qa channel got only its own Q&A' : '\nFAIL: cross-talk or missing Q&A');
  process.exit(pass ? 0 : 1);
})().catch(e => { console.error('FATAL:', e); process.exit(2); });
