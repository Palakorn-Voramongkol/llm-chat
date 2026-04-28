// 4-session end-to-end check via the manager.
// Verification approach (borrowed from origin/linux-port test, adapted):
//   - listen on raw /s/<sid> WS, accumulate every byte claude sends back
//   - strip ANSI escape codes from the captured stream
//   - PASS only if the magic word appears AFTER claude's answer marker `●`
//     (so the user's prompt echo alone doesn't satisfy the check)
//   - additionally check no other session's magic word leaked in
const fs = require('fs');
const os = require('os');
const path = require('path');

const MGR = process.env.MANAGER_URL || 'ws://127.0.0.1:7777';
const TOKEN_PATH = process.env.TOKEN_PATH ||
  path.join(os.homedir(), '.local/share/com.llm-chat.app/auth.token');
const TOKEN = fs.readFileSync(TOKEN_PATH, 'utf8').trim();

const dec = new TextDecoder();
const once = (ws, evt) => new Promise((r) => ws.addEventListener(evt, r, { once: true }));
const readJson = (e) =>
  JSON.parse(typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data)));

function authUrl(p) { return `${MGR}${p}?token=${encodeURIComponent(TOKEN)}`; }

const strip = (s) => s
  .replace(/\x1b\][^\x07]*\x07/g, '')      // OSC ESC ] … BEL
  .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '') // CSI ESC [ … letter
  .replace(/[\x00-\x1f\x7f]/g, ' ')         // remaining control chars
  .replace(/ +/g, ' ');

async function ctrlOpen(ctl) {
  ctl.send(JSON.stringify({ cmd: 'open' }));
  while (true) {
    const ev = await once(ctl, 'message');
    const j = readJson(ev);
    if (j.sessionId) return j; // {ok, sessionId, backendPort}
    if (j.ok === false) throw new Error('open failed: ' + JSON.stringify(j));
  }
}

async function ctrlClose(ctl, sid) {
  ctl.send(JSON.stringify({ cmd: 'close', sessionId: sid }));
  await Promise.race([once(ctl, 'message'), new Promise((r) => setTimeout(r, 2000))]);
}

function sendPromptAndCapture(label, sid, prompt, settleMs, captureMs) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(authUrl(`/s/${sid}`));
    ws.binaryType = 'arraybuffer';
    let raw = '';
    ws.addEventListener('message', (e) => {
      raw += typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data));
    });
    // also subscribe to /qa/<sid> in parallel to capture parsed Q&A events
    const qa = new WebSocket(authUrl(`/qa/${sid}`));
    const qaEvents = [];
    qa.addEventListener('message', (e) => {
      try {
        const j = JSON.parse(typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data)));
        if (j && j.num !== undefined) qaEvents.push(j);
      } catch {}
    });

    ws.addEventListener('error', (e) => reject(new Error(`${label} ws error: ${e.message || e}`)));
    ws.addEventListener('open', async () => {
      // wait for claude TUI to render its initial banner before sending
      await new Promise((r) => setTimeout(r, settleMs));
      console.log(`[${label}] sending: ${prompt}`);
      ws.send(prompt + '\r');
      await new Promise((r) => setTimeout(r, captureMs));
      try { ws.close(); } catch {}
      try { qa.close(); } catch {}
      resolve({ raw, qaEvents });
    });
  });
}

(async () => {
  console.log(`manager=${MGR}  token=${TOKEN.slice(0, 8)}…`);

  const ctl = new WebSocket(authUrl('/control'));
  await once(ctl, 'open');
  await once(ctl, 'message'); // hello

  // Open 4 sessions; manager picks least-loaded backend → expect 2/2 split
  console.log('opening 4 sessions...');
  const opens = [];
  for (let i = 0; i < 4; i++) {
    const r = await ctrlOpen(ctl);
    console.log(`  S${i + 1} → sid=${r.sessionId.slice(0, 14)}…  backend=${r.backendPort}`);
    opens.push(r);
  }

  const byPort = {};
  for (const r of opens) byPort[r.backendPort] = (byPort[r.backendPort] || 0) + 1;
  console.log('distribution:', byPort);
  if (Object.keys(byPort).length !== 2 || Object.values(byPort).some((c) => c !== 2)) {
    console.error('FAIL: expected 2/2 distribution, got', byPort);
    ctl.close();
    process.exit(1);
  }

  // Distinct prompts; verify each session's claude answers with its OWN word
  // and not any other session's word.
  const prompts = [
    { label: 'S1', sid: opens[0].sessionId, magic: 'MAGNOLIA1' },
    { label: 'S2', sid: opens[1].sessionId, magic: 'TANGERINE2' },
    { label: 'S3', sid: opens[2].sessionId, magic: 'HEMLOCK3' },
    { label: 'S4', sid: opens[3].sessionId, magic: 'PISTACHIO4' },
  ];
  for (const p of prompts) p.prompt = `reply with only the single word ${p.magic}`;

  // Run all four chats in parallel: each waits 8s for banner, sends, captures 70s.
  const SETTLE_MS = 8000;
  const CAPTURE_MS = 70000;
  const captures = await Promise.all(
    prompts.map((p) => sendPromptAndCapture(p.label, p.sid, p.prompt, SETTLE_MS, CAPTURE_MS))
  );

  console.log('\n=== per-session verification ===');
  let pass = true;
  const allMagics = prompts.map((p) => p.magic);
  for (let i = 0; i < prompts.length; i++) {
    const p = prompts[i];
    const { raw, qaEvents } = captures[i];
    const cleaned = strip(raw);
    // Look for claude's answer marker `●` followed by the magic word — proves
    // claude actually answered, not just the prompt echo.
    const answerRegex = new RegExp(`●\\s*${p.magic}`, 'i');
    const ownAnswered = answerRegex.test(cleaned);
    // Echo+answer should both contain the word: count must be ≥2
    const occurrences = (cleaned.match(new RegExp(p.magic, 'g')) || []).length;
    // Cross-talk: any OTHER magic word appearing in this session's stream
    const foreign = allMagics
      .filter((m) => m !== p.magic)
      .filter((m) => cleaned.includes(m));
    // /qa/ check: parsed Q&A event with magic word in answer, scoped to this sid
    const qaOk = qaEvents.some((e) =>
      e.sessionId === p.sid && (e.answer || '').includes(p.magic)
    );

    const ok = ownAnswered && occurrences >= 2 && foreign.length === 0 && qaOk;
    if (!ok) pass = false;
    console.log(`[${ok ? 'PASS' : 'FAIL'}] ${p.label} sid=${p.sid.slice(0, 14)}… ` +
      `magic=${p.magic} /s/=${raw.length}b answerMarker=${ownAnswered} ` +
      `occ=${occurrences} foreign=${JSON.stringify(foreign)} ` +
      `/qa/=${qaEvents.length}ev qaOk=${qaOk}`);
    if (!ok) {
      console.log(`        qa events: ${JSON.stringify(qaEvents.map((e) => ({ sid: e.sessionId, a: (e.answer || '').slice(0, 40) })))}`);
    }
  }

  console.log('\nclosing sessions...');
  for (const r of opens) await ctrlClose(ctl, r.sessionId);
  ctl.close();

  console.log(pass
    ? '\n✓ PASS — 4 sessions, 4 distinct claude answers, no cross-talk'
    : '\n✗ FAIL');
  process.exit(pass ? 0 : 1);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
