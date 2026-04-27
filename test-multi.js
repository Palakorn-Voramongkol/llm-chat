// End-to-end test of multi-session via WebSocket.
// 1. Connect to /control, send {cmd:"open"} -> get a new sessionId
// 2. Wait for the PTY to settle, send a chat message via /s/<sessionId>
// 3. Read the binary response stream from Claude
// 4. Also send a message to session 1 in parallel to confirm both sessions work
const decoder = new TextDecoder();

function testSession(label, urlPath, message, totalMs = 15000) {
  return new Promise((resolve) => {
    const url = `ws://127.0.0.1:7878${urlPath}`;
    console.log(`[${label}] connecting to ${url}`);
    const ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';
    let bytes = 0;
    let textCaptured = '';
    ws.addEventListener('open', () => {
      console.log(`[${label}] open; sending ${JSON.stringify(message)}`);
      ws.send(message + '\r');
    });
    ws.addEventListener('message', (e) => {
      const isText = typeof e.data === 'string';
      const txt = isText ? e.data : decoder.decode(new Uint8Array(e.data));
      bytes += txt.length;
      textCaptured += txt;
    });
    ws.addEventListener('error', (err) => {
      console.error(`[${label}] error: ${err.message}`);
      resolve({ label, bytes, textCaptured, ok: false });
    });
    setTimeout(() => {
      ws.close();
      // Strip ANSI to make output more readable; just print first 600 chars
      const stripped = textCaptured.replace(/\x1b\[[0-9;<>?]*[a-zA-Z]/g, '').replace(/\x1b\][^\x07]*\x07/g, '');
      console.log(`[${label}] received ${bytes} bytes total; cleaned snippet:`);
      console.log(stripped.replace(/[\x00-\x1f]/g, ' ').slice(0, 600));
      resolve({ label, bytes, textCaptured, ok: bytes > 0 });
    }, totalMs);
  });
}

async function openNewSession() {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket('ws://127.0.0.1:7878/control');
    ws.addEventListener('open', () => ws.send(JSON.stringify({ cmd: 'open' })));
    let firstNonHelloSeen = false;
    ws.addEventListener('message', (e) => {
      const text = typeof e.data === 'string' ? e.data : decoder.decode(new Uint8Array(e.data));
      console.log('[control]', text);
      try {
        const j = JSON.parse(text);
        if (j.sessionId) {
          firstNonHelloSeen = true;
          ws.close();
          resolve(j.sessionId);
        }
      } catch {}
    });
    ws.addEventListener('error', (err) => reject(err));
    setTimeout(() => { if (!firstNonHelloSeen) reject(new Error('control timeout')); ws.close(); }, 5000);
  });
}

(async () => {
  console.log('=== spawning a new session via /control ===');
  const newId = await openNewSession();
  console.log('new sessionId:', newId);

  // Give the PTY a moment to print its banner
  await new Promise((r) => setTimeout(r, 1500));

  console.log('=== chatting with session 1 (existing) and the new session in parallel ===');
  const [r1, r2] = await Promise.all([
    testSession('s1', '/s/1', 'reply with the word PINEAPPLE', 18000),
    testSession('new', `/s/${newId}`, 'reply with the word ELEPHANT', 18000),
  ]);

  console.log('---');
  console.log('s1 ok:', r1.ok, '/ bytes:', r1.bytes);
  console.log('new ok:', r2.ok, '/ bytes:', r2.bytes);
  console.log('s1 mentions PINEAPPLE:', r1.textCaptured.includes('PINEAPPLE'));
  console.log('new mentions ELEPHANT:', r2.textCaptured.includes('ELEPHANT'));
  process.exit(0);
})().catch((e) => { console.error('FATAL:', e); process.exit(1); });
