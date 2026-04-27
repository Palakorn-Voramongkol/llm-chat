// Exercise the full /control surface end-to-end:
//   list, open, switch, current, history, clear, close
const decoder = new TextDecoder();
function once(ws, evt) { return new Promise((r) => ws.addEventListener(evt, r, { once: true })); }
function readJson(e) { return JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data))); }
async function ctl(ws, cmd) {
  ws.send(JSON.stringify(cmd));
  return readJson(await once(ws, 'message'));
}
function chat(sid, magic, ms = 18000) {
  return new Promise((res) => {
    const ws = new WebSocket(`ws://127.0.0.1:7878/s/${sid}`);
    ws.binaryType = 'arraybuffer';
    ws.addEventListener('open', () => ws.send(`reply with the single word ${magic}\r`));
    setTimeout(() => { ws.close(); res(); }, ms);
  });
}
function ok(label, cond) {
  console.log(`[${cond ? 'OK' : 'FAIL'}] ${label}`);
  return cond;
}

(async () => {
  const w = new WebSocket('ws://127.0.0.1:7878/control');
  await once(w, 'open');
  await once(w, 'message');

  // initial list
  let r = await ctl(w, { cmd: 'list' });
  console.log('initial list:', r);
  const initialCount = r.sessions.length;

  // open two extra sessions
  const a = (await ctl(w, { cmd: 'open' })).sessionId;
  const b = (await ctl(w, { cmd: 'open' })).sessionId;
  await new Promise((res) => setTimeout(res, 1500));

  r = await ctl(w, { cmd: 'list' });
  let pass = ok(`list has ${initialCount + 2} sessions after 2 opens`, r.sessions.length === initialCount + 2);

  // chat with each new session
  await Promise.all([
    chat(a, 'GINGER'),
    chat(b, 'PEPPERMINT'),
  ]);

  // history for session A should mention GINGER
  r = await ctl(w, { cmd: 'history', sessionId: a });
  pass = ok('history(a) mentions GINGER', r.history.some(x => x.answer.includes('GINGER'))) && pass;
  pass = ok('history(a) does NOT contain PEPPERMINT', !r.history.some(x => x.answer.includes('PEPPERMINT'))) && pass;
  r = await ctl(w, { cmd: 'history', sessionId: b });
  pass = ok('history(b) mentions PEPPERMINT', r.history.some(x => x.answer.includes('PEPPERMINT'))) && pass;
  pass = ok('history(b) does NOT contain GINGER', !r.history.some(x => x.answer.includes('GINGER'))) && pass;

  // switch the GUI to session A
  r = await ctl(w, { cmd: 'switch', sessionId: a });
  pass = ok('switch reply ok', r.ok) && pass;
  await new Promise((res) => setTimeout(res, 300));
  r = await ctl(w, { cmd: 'current' });
  pass = ok('current reports session A active', r.active === a) && pass;

  // clear stream of session B
  r = await ctl(w, { cmd: 'clear', sessionId: b, what: 'stream' });
  pass = ok('clear stream replies ok', r.ok) && pass;
  await new Promise((res) => setTimeout(res, 200));
  r = await ctl(w, { cmd: 'history', sessionId: b });
  pass = ok('history(b) is empty after clear stream', r.history.length === 0) && pass;
  // session A should still have history
  r = await ctl(w, { cmd: 'history', sessionId: a });
  pass = ok('history(a) survived clear of b', r.history.length > 0) && pass;

  // close session A
  r = await ctl(w, { cmd: 'close', sessionId: a });
  pass = ok('close A replies ok', r.ok) && pass;
  await new Promise((res) => setTimeout(res, 300));
  r = await ctl(w, { cmd: 'list' });
  pass = ok('list no longer contains A', !r.sessions.includes(a)) && pass;
  pass = ok('list still contains B', r.sessions.includes(b)) && pass;

  // history of A should be gone
  r = await ctl(w, { cmd: 'history', sessionId: a });
  pass = ok('history(a) empty after close', r.history.length === 0) && pass;

  // close session B too
  r = await ctl(w, { cmd: 'close', sessionId: b });
  pass = ok('close B replies ok', r.ok) && pass;
  r = await ctl(w, { cmd: 'list' });
  pass = ok(`list back to ${initialCount}`, r.sessions.length === initialCount) && pass;

  w.close();
  console.log(pass ? '\nPASS overall' : '\nFAIL overall');
  process.exit(pass ? 0 : 1);
})().catch(e => { console.error('FATAL:', e); process.exit(2); });
