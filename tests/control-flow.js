// Full multi-session + control-channel exercise:
//  1. List existing sessions
//  2. Open 2 brand-new sessions via /control "open"
//  3. Send a unique magic word to every session in parallel
//  4. Use /control "switch" to bring the second new session to focus in the GUI
//  5. Report PASS/FAIL per session
const decoder = new TextDecoder();
const strip = (s) => s
  .replace(/\x1b\][^\x07]*\x07/g, '')
  .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
  .replace(/[\x00-\x1f\x7f]/g, ' ')
  .replace(/ +/g, ' ');

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
async function ctrlSwitch(ctl, sid) {
  ctl.send(JSON.stringify({ cmd: 'switch', sessionId: sid }));
  while (true) {
    const ev = await once(ctl, 'message');
    const j = readJson(ev);
    if (j.sessionId === sid || j.error) return j;
  }
}

function chat(label, sid, magic, ms = 22000) {
  return new Promise((resolve) => {
    const ws = new WebSocket(`ws://127.0.0.1:7878/s/${sid}`);
    ws.binaryType = 'arraybuffer';
    let raw = '';
    ws.addEventListener('open', () => ws.send(`reply with the single word ${magic}\r`));
    ws.addEventListener('message', (e) => {
      raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
    });
    setTimeout(() => {
      ws.close();
      resolve({ label, sid, magic, raw, cleaned: strip(raw) });
    }, ms);
  });
}

(async () => {
  // open control channel
  const ctl = new WebSocket('ws://127.0.0.1:7878/control');
  await once(ctl, 'open');
  await once(ctl, 'message'); // hello banner

  // spawn two new sessions via control
  console.log('opening 2 new sessions via /control...');
  const newA = await ctrlOpen(ctl);
  console.log(' →', newA);
  const newB = await ctrlOpen(ctl);
  console.log(' →', newB);

  // wait for PTYs to print their banners
  await new Promise((r) => setTimeout(r, 1500));

  // every session in the system gets a unique magic word
  const sessionsList = await new Promise((res) => {
    const w = new WebSocket('ws://127.0.0.1:7878/');
    w.addEventListener('message', (e) => { res(readJson(e)); w.close(); });
  });
  console.log('all sessions:', sessionsList);

  const MAGIC = ['MARMALADE', 'BUTTERSCOTCH', 'AVOCADO', 'SNORKEL', 'TANGERINE', 'KEROSENE', 'WALNUT', 'PRETZEL'];
  const results = await Promise.all(
    sessionsList.map((sid, i) => chat(`session#${i + 1}`, sid, MAGIC[i % MAGIC.length]))
  );

  console.log('\n=== per-session response check ===');
  let pass = true;
  for (const r of results) {
    const ok = r.cleaned.includes(r.magic);
    if (!ok) pass = false;
    // tail of cleaned response so user can read it
    const tail = r.cleaned.slice(-300);
    console.log(`[${ok ? 'OK' : 'FAIL'}] ${r.label} (${r.sid.slice(0, 22)}…) sent "${r.magic}"; tail=…${JSON.stringify(tail)}`);
  }

  // ask GUI to switch to the second new session
  console.log('\n=== /control switch to', newB, '===');
  const switchReply = await ctrlSwitch(ctl, newB);
  console.log('switch reply:', switchReply);
  ctl.close();

  console.log('\n', pass ? 'PASS overall (each session got its own magic word)' : 'FAIL overall');
  process.exit(pass ? 0 : 1);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
