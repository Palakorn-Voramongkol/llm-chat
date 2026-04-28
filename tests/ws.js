// Tests the per-session WebSocket relay.
//   node test-ws.js <sessionIndex> <messageToSend>
// Requires Node 22+ (built-in WebSocket).
const idx = process.argv[2] || '1';
const msg = process.argv[3] || 'hello';
const url = `ws://127.0.0.1:7878/s/${idx}`;
console.log(`[client] connecting to ${url}`);
const ws = new WebSocket(url);
ws.binaryType = 'arraybuffer';
let got = 0;
ws.addEventListener('open', () => {
  console.log('[client] open; sending', JSON.stringify(msg + '\r'));
  ws.send(msg + '\r');
});
ws.addEventListener('message', (e) => {
  got++;
  const text = typeof e.data === 'string'
    ? e.data
    : new TextDecoder().decode(new Uint8Array(e.data));
  process.stdout.write(`[s${idx}] ${JSON.stringify(text)}\n`);
});
ws.addEventListener('error', (e) => console.error('[client] error:', e.message));
ws.addEventListener('close', (e) => {
  console.log(`[client] closed (${e.code}); got ${got} messages`);
  process.exit(0);
});
setTimeout(() => {
  console.log('[client] 12s timeout; closing');
  ws.close();
}, 12000);
