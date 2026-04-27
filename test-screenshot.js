// Trigger screenshot capture on every backend via the manager's /control,
// print the resulting PNG paths.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const tokenPath = path.join(os.tmpdir(), 'llm-chat-qa', 'auth.token');
const token = readFileSync(tokenPath, 'utf8').trim();

const ws = new WebSocket(`ws://127.0.0.1:7777/control?token=${token}`);
ws.addEventListener('open', () => {
  ws.send(JSON.stringify({ cmd: 'screenshot' }));
});
let replies = 0;
ws.addEventListener('message', (e) => {
  const j = JSON.parse(typeof e.data === 'string' ? e.data : new TextDecoder().decode(e.data));
  console.log(JSON.stringify(j, null, 2));
  if (replies++ >= 1) ws.close();
});
ws.addEventListener('error', (e) => {
  console.error('ws error', e.message || e);
  process.exit(1);
});
ws.addEventListener('close', () => process.exit(0));
