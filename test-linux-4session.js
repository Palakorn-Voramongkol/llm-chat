// 4-session distinct-routing test for the Linux Docker manager.
// Manager spawns 2 backends, each backend auto-spawns 1 session at startup
// (= 2 sessions). We open 2 more via /control to bring the total to 4
// (round-robined → 2 per backend). Then we send each session a unique magic
// word and assert each session's response contains its own word.
//
// Each Claude Code instance, when started with --dangerously-skip-permissions,
// shows a "Bypass Permissions mode — accept?" prompt BEFORE the chat prompt.
// We auto-dismiss that by sending "2\r" (select "Yes, I accept") with a delay.

const TOKEN = process.env.LLM_CHAT_TOKEN;
if (!TOKEN) { console.error('set LLM_CHAT_TOKEN'); process.exit(2); }
const PORT = process.env.MANAGER_PORT || 7777;
const Q = `?token=${encodeURIComponent(TOKEN)}`;
const decoder = new TextDecoder();

const MAGIC = ['MAGNOLIA1', 'TANGERINE2', 'HEMLOCK3', 'PISTACHIO4'];

const strip = (s) => s
    .replace(/\x1b\][^\x07]*\x07/g, '')
    .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
    .replace(/[\x00-\x1f\x7f]/g, ' ')
    .replace(/ +/g, ' ');

function once(ws, evt) { return new Promise((r, j) => {
    ws.addEventListener(evt, r, { once: true });
    ws.addEventListener('error', j, { once: true });
}); }
function readJson(e) { return JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data))); }
async function ctrl(ws, cmd) { ws.send(JSON.stringify(cmd)); return readJson(await once(ws, 'message')); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Dance through Claude's first-run prompts: (1) "trust this folder" — default
// is option 1 (Yes), so Enter accepts; (2) "Bypass Permissions warning" —
// default is option 1 (No, exit), so we arrow-down then Enter to pick "Yes,
// I accept". Then send the real question.
async function chat(label, sid, magic, totalMs = 75000) {
    return new Promise(async (resolve) => {
        const ws = new WebSocket(`ws://127.0.0.1:${PORT}/s/${sid}${Q}`);
        ws.binaryType = 'arraybuffer';
        let raw = '';
        ws.addEventListener('message', (e) => {
            raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
        });
        await once(ws, 'open');
        await sleep(8000);             // wait for claude to render the trust prompt
        ws.send('\r');                 // accept "Yes, I trust this folder"
        await sleep(2500);             // bypass-perms warning appears
        ws.send('\x1b[B');             // arrow-down → cursor on "Yes, I accept"
        await sleep(700);
        ws.send('\r');                 // confirm
        await sleep(2500);             // claude main UI loads
        ws.send(`reply with the single word ${magic}\r`);
        const remaining = Math.max(15000, totalMs - 13700);
        await sleep(remaining);
        ws.close();
        const cleaned = strip(raw);
        const sawMagic = cleaned.includes(magic);
        resolve({ label, sid, magic, raw, cleaned, sawMagic });
    });
}

(async () => {
    const cw = new WebSocket(`ws://127.0.0.1:${PORT}/control${Q}`);
    await once(cw, 'open');
    await once(cw, 'message');

    const before = await ctrl(cw, { cmd: 'list' });
    console.log(`existing sessions: ${before.count} (expected 0 in managed mode)`);

    const opened = [];
    for (let i = 0; i < 4; i++) {
        const r = await ctrl(cw, { cmd: 'open' });
        if (!r.ok) { console.error('open failed', r); process.exit(1); }
        opened.push({ sid: r.sessionId, port: r.backendPort });
        console.log(`opened ${r.sessionId} on backend port ${r.backendPort}`);
        // Stagger startups so the 4 claude processes don't race on
        // `~/.claude.json` while they initialise.
        await sleep(6000);
    }

    const list = await ctrl(cw, { cmd: 'list' });
    console.log(`total sessions: ${list.count}`);
    console.log(`per backend: ${JSON.stringify(Object.fromEntries(Object.entries(list.byBackend).map(([k,v])=>[k,v.count])))}`);

    console.log('sending 4 distinct prompts in parallel (each session does its own keystroke dance)...');
    const results = await Promise.all(
        opened.map((o, i) => chat(`s${i+1}`, o.sid, MAGIC[i], 75000))
    );

    let pass = true;
    console.log('\n=== summary ===');
    for (const r of results) {
        const status = r.sawMagic ? 'PASS' : 'FAIL';
        if (!r.sawMagic) pass = false;
        console.log(`[${status}] ${r.label} (${r.sid.slice(0,28)}...) magic="${r.magic}"`);
        if (!r.sawMagic) {
            console.log(`        last 300 chars: ${r.cleaned.slice(-300)}`);
        }
    }

    // Close only the sessions we opened (leave auto-spawn ones)
    for (const o of opened) await ctrl(cw, { cmd: 'close', sessionId: o.sid });
    cw.close();

    console.log(pass ? '\n✓ ALL 4 sessions answered with their own magic word'
                     : '\n✗ at least one session did not echo its magic word');
    process.exit(pass ? 0 : 1);
})().catch((e) => { console.error('FATAL', e); process.exit(2); });
