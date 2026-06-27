// Verify the selectable rich-output mode on the stream-json transport.
//
// Opens THREE sessions on one worker — rich=off, rich=turn, rich=token — sends
// each the same tool-using prompt, subscribes to each /qa channel, and checks:
//   off   → only the final `result` line (num + final:true), NO rich events
//   turn  → assistant + user(tool_result) events, NO stream_event, then final
//   token → additionally stream_event deltas, then final
// and that the final `result` payload is intact for all three (backward compat).
//
// Env: PORT (default 7879), TOKEN (required), USERID (default 'tester').
// Run:  PORT=7879 TOKEN=<32+ hex> node tests/rich-stream.js

const PORT = process.env.PORT || '7879';
const TOKEN = process.env.TOKEN;
const USERID = process.env.USERID || 'tester';
if (!TOKEN) { console.error('TOKEN env is required'); process.exit(2); }

const HOST = `127.0.0.1:${PORT}`;
const url = (path) => `ws://${HOST}${path}?token=${encodeURIComponent(TOKEN)}`;
const decoder = new TextDecoder();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const once = (ws, evt) => new Promise((res) => ws.addEventListener(evt, res, { once: true }));
const readJson = (e) => {
  const t = typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
  return JSON.parse(t);
};

const LEVELS = [
  { level: 'off', magic: 'hello_off_42' },
  { level: 'turn', magic: 'hello_turn_42' },
  { level: 'token', magic: 'hello_token_42' },
];

// Send one {cmd:open, userId, rich} and read /control replies until the one
// carrying our sessionId (skips the connect greeting + any other lines).
async function ctrlOpen(ctl, level, userId) {
  ctl.send(JSON.stringify({ cmd: 'open', userId, rich: level }));
  while (true) {
    const ev = await once(ctl, 'message');
    let j; try { j = readJson(ev); } catch { continue; }
    if (j.ok === false) throw new Error(`open rejected (rich=${level}): ${j.error}`);
    if (j.sessionId) return j; // { ok, sessionId, transport, rich }
  }
}

function subscribeQa(sid) {
  const ws = new WebSocket(url(`/qa/${sid}`));
  const events = [];
  ws.addEventListener('message', (e) => {
    try { events.push(readJson(e)); } catch { /* ignore non-JSON banners */ }
  });
  return { ws, events };
}

function drive(sid, magic) {
  const ws = new WebSocket(url(`/s/${sid}`));
  ws.binaryType = 'arraybuffer';
  ws.addEventListener('open', () => {
    // Trailing \r is the submit frame the worker buffers on.
    ws.send(`Use the Bash tool to run exactly: echo ${magic} — then reply with only the word DONE.\r`);
  });
  return ws;
}

(async () => {
  const ctl = new WebSocket(url('/control'));
  await once(ctl, 'open');

  // Open the three sessions (sequentially, one control socket).
  const sessions = [];
  for (const { level, magic } of LEVELS) {
    const reply = await ctrlOpen(ctl, level, USERID);
    console.log(`opened rich=${level.padEnd(5)} → sid=${reply.sessionId}  (reply.rich=${reply.rich}, transport=${reply.transport})`);
    sessions.push({ level, magic, sid: reply.sessionId, reply });
  }
  ctl.close();

  // Subscribe BEFORE driving so we miss nothing.
  for (const s of sessions) s.sub = subscribeQa(s.sid);
  await sleep(1000);

  // Drive all three in parallel; keep /s sockets open through the turn.
  for (const s of sessions) s.drv = drive(s.sid, s.magic);
  console.log('\n…driving tool-using prompts, waiting for turns to complete…\n');
  await sleep(35000);

  for (const s of sessions) { try { s.drv.close(); } catch {} s.sub.ws.close(); }
  await sleep(300);

  // ---- analyze ----
  let allPass = true;
  for (const s of sessions) {
    const evs = s.sub.events;
    const finals = evs.filter((e) => typeof e.num === 'number');
    const rich = evs.filter((e) => e.type === 'event');
    const kinds = {};
    for (const e of rich) kinds[e.kind] = (kinds[e.kind] || 0) + 1;
    const streamDeltas = rich.filter((e) => e.kind === 'stream_event');
    const toolResults = rich.filter(
      (e) => e.kind === 'user' &&
        JSON.stringify(e.raw || '').includes('tool_result'));

    // Reassemble streamed text deltas (token level) as a liveliness proof.
    const typed = streamDeltas
      .map((e) => e.raw?.event?.delta)
      .filter((d) => d && d.type === 'text_delta')
      .map((d) => d.text).join('');

    const finalAnswer = finals.map((f) => f.answer || '').join(' ');
    console.log(`── rich=${s.level} (sid ${s.sid}) ──`);
    console.log(`   reply.rich      = ${s.reply.rich}`);
    console.log(`   final results   = ${finals.length}  (final:true=${finals.every((f) => f.final === true)}, answer="${finalAnswer.slice(0, 60)}")`);
    console.log(`   rich events     = ${rich.length}  kinds=${JSON.stringify(kinds)}`);
    console.log(`   stream deltas   = ${streamDeltas.length}  reassembled text="${typed.slice(0, 60)}"`);
    console.log(`   own magic seen  = ${evs.some((e) => JSON.stringify(e).includes(s.magic))}`);

    // ---- per-level expectations ----
    const checks = [];
    const expect = (name, cond) => { checks.push([name, cond]); if (!cond) allPass = false; };
    expect('reply.rich echoes level', s.reply.rich === s.level);
    expect('exactly one final result', finals.length === 1);
    expect('final has final:true + string answer',
      finals.length === 1 && finals[0].final === true && typeof finals[0].answer === 'string');
    if (s.level === 'off') {
      expect('NO rich events', rich.length === 0);
    } else if (s.level === 'turn') {
      expect('has assistant event(s)', (kinds.assistant || 0) >= 1);
      expect('has tool_result (user) event(s)', toolResults.length >= 1);
      expect('NO stream_event deltas', streamDeltas.length === 0);
    } else if (s.level === 'token') {
      expect('has assistant event(s)', (kinds.assistant || 0) >= 1);
      expect('has stream_event deltas', streamDeltas.length >= 1);
    }
    for (const [name, ok] of checks) console.log(`     ${ok ? 'PASS' : 'FAIL'}  ${name}`);
    console.log('');
  }

  console.log(allPass ? '✅ ALL PASS' : '❌ SOME CHECKS FAILED');
  process.exit(allPass ? 0 : 1);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
