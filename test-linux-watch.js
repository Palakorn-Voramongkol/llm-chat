// Open one session, connect chat WS BEFORE claude starts emitting anything,
// dump everything we receive for 30s. This lets us see exactly what claude
// shows on a fresh PTY.

const TOKEN = process.env.LLM_CHAT_TOKEN;
if (!TOKEN) { process.exit(2); }
const Q = `?token=${encodeURIComponent(TOKEN)}`;
const dec = new TextDecoder();

function once(ws, evt) { return new Promise((r) => ws.addEventListener(evt, r, { once: true })); }
function readJson(e) { return JSON.parse(typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data))); }
async function ctrl(ws, c) { ws.send(JSON.stringify(c)); return readJson(await once(ws, 'message')); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
    const cw = new WebSocket(`ws://127.0.0.1:7777/control${Q}`);
    await once(cw, 'open'); await once(cw, 'message');
    const r = await ctrl(cw, { cmd: 'open' });
    const sid = r.sessionId;
    console.log(`opened ${sid} on backend ${r.backendPort}`);

    // Connect chat WS IMMEDIATELY - claude takes ~5-8s to spawn so we'll be
    // subscribed before any output happens.
    const ws = new WebSocket(`ws://127.0.0.1:7777/s/${sid}${Q}`);
    ws.binaryType = 'arraybuffer';
    let raw = '';
    let lastChunkAt = Date.now();
    ws.addEventListener('message', (e) => {
        const chunk = typeof e.data === 'string' ? e.data : dec.decode(new Uint8Array(e.data));
        raw += chunk;
        lastChunkAt = Date.now();
    });
    await once(ws, 'open');
    console.log('chat ws subscribed; waiting 15s for claude to start...');
    await sleep(15000);
    console.log(`raw len=${raw.length}, last chunk ${Date.now()-lastChunkAt}ms ago`);
    console.log('--- raw output (stripped) ---');
    const cleaned = raw
        .replace(/\x1b\][^\x07]*\x07/g, '')
        .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
        .replace(/[\x00-\x1f\x7f]/g, ' ')
        .replace(/ +/g, ' ');
    console.log(cleaned);

    console.log('\n--- sending Enter (default: trust folder = yes) ---');
    ws.send('\r');
    await sleep(3000);
    console.log(`raw len after first Enter: ${raw.length}`);
    console.log('\n--- try arrow-down (CSI B) ---');
    ws.send('\x1b[B');
    await sleep(1500);
    console.log(`after arrow-down: raw len=${raw.length}`);
    const cAD = raw.replace(/\x1b\][^\x07]*\x07/g,'').replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g,'').replace(/[\x00-\x1f\x7f]/g,' ').replace(/ +/g,' ');
    console.log('  marker:', cAD.includes('Yes,Iaccept') && cAD.lastIndexOf('❯') > cAD.indexOf('Yes,Iaccept') - 30 ? 'cursor on Yes' : 'cursor still on No');
    console.log('\n--- send Enter to confirm ---');
    ws.send('\r');
    await sleep(3000);
    console.log(`raw len=${raw.length} after dismiss`);
    const c2 = raw
        .replace(/\x1b\][^\x07]*\x07/g, '')
        .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
        .replace(/[\x00-\x1f\x7f]/g, ' ')
        .replace(/ +/g, ' ');
    console.log('last 500 chars after dismiss:', c2.slice(-500));

    console.log('\n--- sending question ---');
    ws.send('reply with the single word PINEAPPLE7\r');
    await sleep(30000);
    const c3 = raw
        .replace(/\x1b\][^\x07]*\x07/g, '')
        .replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '')
        .replace(/[\x00-\x1f\x7f]/g, ' ')
        .replace(/ +/g, ' ');
    console.log(`raw len=${raw.length} after question`);
    console.log('last 800 chars:', c3.slice(-800));
    console.log('\nsaw PINEAPPLE7:', c3.includes('PINEAPPLE7'));

    ws.close();
    await ctrl(cw, { cmd: 'close', sessionId: sid });
    cw.close();
    process.exit(0);
})().catch((e) => { console.error('FATAL', e); process.exit(2); });
