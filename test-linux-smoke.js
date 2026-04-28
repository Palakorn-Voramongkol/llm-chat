// Linux/Docker smoke test. Pass the auth token via env LLM_CHAT_TOKEN
// (read from inside the container with `docker exec ... cat
// /root/.local/share/com.llm-chat.app/auth.token`).
//
// Verifies: manager /control responds, opens a session, lists 3 sessions
// (1 auto-created on each backend + 1 manager-opened), closes the manager-
// opened session, then count drops to 2 (the two auto-created backend ones).

const TOKEN = process.env.LLM_CHAT_TOKEN;
if (!TOKEN) {
    console.error('set LLM_CHAT_TOKEN');
    process.exit(2);
}
const MANAGER_PORT = process.env.MANAGER_PORT || 7777;
const Q = `?token=${encodeURIComponent(TOKEN)}`;

const decoder = new TextDecoder();
function once(ws, evt) { return new Promise((r, j) => {
    ws.addEventListener(evt, r, { once: true });
    ws.addEventListener('error', j, { once: true });
}); }
function readJson(e) {
    return JSON.parse(typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data)));
}
async function ctrl(ws, cmd) {
    ws.send(JSON.stringify(cmd));
    return readJson(await once(ws, 'message'));
}

(async () => {
    const url = `ws://127.0.0.1:${MANAGER_PORT}/control${Q}`;
    console.log('connecting to', url);
    const ws = new WebSocket(url);
    await once(ws, 'open');
    const hello = await once(ws, 'message');
    console.log('hello:', typeof hello.data === 'string' ? hello.data : decoder.decode(new Uint8Array(hello.data)));

    const before = await ctrl(ws, { cmd: 'list' });
    console.log('list (before open):', JSON.stringify(before));

    const open = await ctrl(ws, { cmd: 'open' });
    console.log('open:', JSON.stringify(open));
    if (!open.ok || !open.sessionId) { console.error('FAIL open'); process.exit(1); }

    const after = await ctrl(ws, { cmd: 'list' });
    console.log('list (after open):', JSON.stringify(after));
    if (after.count !== before.count + 1) {
        console.error(`FAIL: expected count=${before.count + 1}, got ${after.count}`);
        process.exit(1);
    }

    const inst = await ctrl(ws, { cmd: 'instances' });
    console.log('instances:', JSON.stringify(inst));

    const close = await ctrl(ws, { cmd: 'close', sessionId: open.sessionId });
    console.log('close:', JSON.stringify(close));

    const final = await ctrl(ws, { cmd: 'list' });
    console.log('list (after close):', JSON.stringify(final));
    if (final.count !== before.count) {
        console.error(`FAIL: expected count back to ${before.count}, got ${final.count}`);
        process.exit(1);
    }

    ws.close();
    console.log('PASS: linux manager open/list/close cycle works');
    process.exit(0);
})().catch((e) => { console.error('FATAL:', e); process.exit(2); });
