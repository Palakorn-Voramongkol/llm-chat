# buffer-debug

**Diagnostic, not pass/fail.** Like `raw-stream-debug` but waits longer
(60s after sending the prompt) and counts the parser's marker characters
in the captured stream:

| count | meaning |
|---|---|
| `> (gt)` and `❯ (bold gt)` | how often a question prompt was rendered |
| `● (bullet U+25CF)` | how often claude's answer marker was rendered |
| `<MAGIC>` (target word) | how often the magic word appeared total (echo + answer) |
| `Welcome` / `tip` | claude's banner / shortcut line — confirms TUI rendered |

Also writes the full raw bytes to `/tmp/claude-raw.bin` and the ANSI-stripped
text to `/tmp/claude-stripped.txt` for offline inspection.

## Setup

Manager up:

```bash
setsid env MANAGER_STEALTH=1 MANAGER_INSTANCES=1 \
  ./manager/target/release/llm-chat-manager \
  > /tmp/manager.log 2>&1 < /dev/null &
disown
sleep 14
```

## Run

```bash
node testcases/e2e/cases/buffer-debug/run.cjs
```

Expected pattern when claude's TUI is healthy:

```
> (gt): 3
❯ (bold gt): 7
● (bullet U+25CF): 2
ALPHA (target word): 4
```

If `●` count is 0, claude rendered the question echo but never produced an
answer — most often a startup race (extend the pre-prompt sleep).
