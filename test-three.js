// Spawn 2 sessions on top of the auto-spawned one (=> 3 total), test each
// gets its own response, verify /control "current" returns the active id and
// that "switch" actually moves it.
const decoder = new TextDecoder();
const strip = (s) => s
  .replace(/\x1b\][^\x07]*\x07/g, '')
  .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
  .replace(/[\x00-\x1f\x7f]/g, ' ')
  .replace(/ +/g, ' ');

function once(ws, evt) { return new Promise((r) => ws.addEventListener(evt, r, { once: true })); }
function readJson(e) { return JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data))); }

async function ctrlCmd(ctl, cmd) {
  ctl.send(JSON.stringify(cmd));
  const ev = await once(ctl, 'message');
  return readJson(ev);
}

function chat(label, sid, magic, ms = 22000) {
  return new Promise((resolve) => {
    const ws = new WebSocket(`ws://127.0.0.1:7878/s/${sid}`);
    ws.binaryType = 'arraybuffer';
    let raw = '';
    ws.addEventListener('open', () => ws.send(`reply with single word ${magic}\r`));
    ws.addEventListener('message', (e) => {
      raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
    });
    setTimeout(() => {
      ws.close();
      const cleaned = strip(raw);
      resolve({ label, sid, magic, raw, cleaned, mentions: cleaned.includes(magic) });
    }, ms);
  });
}

(async () => {
  const ctl = new WebSocket('ws://127.0.0.1:7878/control');
  await once(ctl, 'open');
  await once(ctl, 'message'); // hello

  // Open 2 fresh sessions in addition to the auto-spawned one
  const a = (await ctrlCmd(ctl, { cmd: 'open' })).sessionId;
  const b = (await ctrlCmd(ctl, { cmd: 'open' })).sessionId;
  await new Promise((r) => setTimeout(r, 1500));

  // List all sessions to know the auto one too
  const list = await ctrlCmd(ctl, { cmd: 'list' });
  console.log('list:', list);
  if (list.sessions.length < 3) { console.error('expected 3 sessions; aborting'); process.exit(2); }

  // current sessionId before switching
  const before = await ctrlCmd(ctl, { cmd: 'current' });
  console.log('current before switch:', before);

  // Send distinct prompts to all three in parallel
  const [r0, r1, r2] = await Promise.all([
    chat('s1', list.sessions[0], 'WALNUT'),
    chat('s2', list.sessions[1], 'PRETZEL'),
    chat('s3', list.sessions[2], 'OLIVE'),
  ]);

  // Switch the GUI to session #2 via control
  console.log('\nswitching GUI to', list.sessions[1]);
  const sw = await ctrlCmd(ctl, { cmd: 'switch', sessionId: list.sessions[1] });
  console.log('switch reply:', sw);
  await new Promise((r) => setTimeout(r, 400));
  const after = await ctrlCmd(ctl, { cmd: 'current' });
  console.log('current after switch:', after);

  ctl.close();

  console.log('\n=== summary ===');
  for (const r of [r0, r1, r2]) {
    console.log(`[${r.mentions ? 'OK' : 'FAIL'}] ${r.label} (${r.sid.slice(0,22)}…) sent "${r.magic}"; got=${r.mentions}`);
  }
  const switchedOk = after.active === list.sessions[1];
  console.log(`switch: GUI now reports active=${after.active} → expected=${list.sessions[1]}: ${switchedOk}`);
  const pass = r0.mentions && r1.mentions && r2.mentions && switchedOk;
  console.log(pass ? '\nPASS' : '\nFAIL');
  process.exit(pass ? 0 : 1);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
