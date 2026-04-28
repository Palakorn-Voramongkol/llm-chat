// Open a session through the Docker manager, send Claude a magic-word prompt
// over /s/<sessionId>, wait for the answer, assert the magic word echoes back.

const TOKEN = process.env.LLM_CHAT_TOKEN;
if (!TOKEN) { console.error('set LLM_CHAT_TOKEN'); process.exit(2); }
const PORT = process.env.MANAGER_PORT || 7777;
const Q = `?token=${encodeURIComponent(TOKEN)}`;
const decoder = new TextDecoder();
const MAGIC = 'PINEAPPLE7';

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

(async () => {
    const ctrlWs = new WebSocket(`ws://127.0.0.1:${PORT}/control${Q}`);
    await once(ctrlWs, 'open');
    await once(ctrlWs, 'message'); // hello

    const open = await ctrl(ctrlWs, { cmd: 'open' });
    if (!open.ok) { console.error('open failed', open); process.exit(1); }
    const sid = open.sessionId;
    console.log(`session opened: ${sid} on backend port ${open.backendPort}`);

    console.log('warming up Claude (10s)...');
    await new Promise((r) => setTimeout(r, 10000));

    const chatWs = new WebSocket(`ws://127.0.0.1:${PORT}/s/${sid}${Q}`);
    chatWs.binaryType = 'arraybuffer';
    let raw = '';
    chatWs.addEventListener('message', (e) => {
        raw += typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
    });
    await once(chatWs, 'open');
    console.log('chat ws connected, dismissing bypass-perms prompt...');
    chatWs.send('2\r');
    await new Promise((r) => setTimeout(r, 3000));
    console.log(`(after dismiss, raw len=${raw.length})`);
    console.log('sending question...');
    chatWs.send(`reply with the single word ${MAGIC}\r`);
    // Wait up to 60s for Claude to respond.
    await new Promise((r) => setTimeout(r, 60000));
    chatWs.close();

    const cleaned = strip(raw);
    const sawMagic = cleaned.includes(MAGIC);
    console.log(`raw bytes: ${raw.length}, cleaned: ${cleaned.length}, saw "${MAGIC}": ${sawMagic}`);
    if (!sawMagic) {
        console.log('--- last 600 chars of cleaned response ---');
        console.log(cleaned.slice(-600));
    }

    await ctrl(ctrlWs, { cmd: 'close', sessionId: sid });
    ctrlWs.close();

    console.log(sawMagic ? '\nPASS: Linux Docker Claude responds to chat' : '\nFAIL');
    process.exit(sawMagic ? 0 : 1);
})().catch((e) => { console.error('FATAL', e); process.exit(2); });
