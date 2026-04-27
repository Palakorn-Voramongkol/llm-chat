// Spawn 2 sessions, send a different prompt to each, then fetch the
// per-session Q&A history via /control "history" and show them.
const decoder = new TextDecoder();
function once(ws, evt) { return new Promise((r) => ws.addEventListener(evt, r, { once: true })); }
function readJson(e) { return JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data))); }
async function ctrlCmd(ctl, cmd) {
  ctl.send(JSON.stringify(cmd));
  const ev = await once(ctl, 'message');
  return readJson(ev);
}
function chat(sid, magic, ms = 18000) {
  return new Promise((res) => {
    const ws = new WebSocket(`ws://127.0.0.1:7878/s/${sid}`);
    ws.binaryType = 'arraybuffer';
    ws.addEventListener('open', () => ws.send(`reply with single word ${magic}\r`));
    setTimeout(() => { ws.close(); res(); }, ms);
  });
}

(async () => {
  const ctl = new WebSocket('ws://127.0.0.1:7878/control');
  await once(ctl, 'open');
  await once(ctl, 'message');

  const a = (await ctrlCmd(ctl, { cmd: 'open' })).sessionId;
  const b = (await ctrlCmd(ctl, { cmd: 'open' })).sessionId;
  await new Promise((r) => setTimeout(r, 1500));

  const list = await ctrlCmd(ctl, { cmd: 'list' });
  console.log('list:', list);

  await Promise.all([
    chat(list.sessions[0], 'GINGER'),
    chat(list.sessions[1], 'PEPPERMINT'),
    chat(list.sessions[2], 'CARDAMOM'),
  ]);
  await new Promise((r) => setTimeout(r, 1500));

  // Per-session history
  for (let i = 1; i <= list.sessions.length; i++) {
    const h = await ctrlCmd(ctl, { cmd: 'history', sessionId: String(i) });
    console.log(`\nhistory s${i} (${h.sessionId.slice(0, 22)}…):`);
    for (const item of h.history) {
      console.log(`  Q${item.num}: ${item.question}`);
      console.log(`  A${item.num}: ${item.answer}`);
    }
  }

  // Bulk history
  const bulk = await ctrlCmd(ctl, { cmd: 'history' });
  console.log('\nbulk keys:', Object.keys(bulk.histories || {}));

  ctl.close();
  process.exit(0);
})().catch(e => { console.error('FATAL:', e); process.exit(1); });
