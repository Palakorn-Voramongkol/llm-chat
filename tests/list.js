const ws = new WebSocket('ws://127.0.0.1:7878/');
ws.addEventListener('open', () => console.log('[client] open'));
ws.addEventListener('message', (e) => {
  console.log('[server] sessions:', e.data);
  ws.close();
});
ws.addEventListener('error', (e) => console.error('error:', e.message));
ws.addEventListener('close', () => process.exit(0));
setTimeout(() => process.exit(1), 4000);
