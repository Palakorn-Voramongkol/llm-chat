#!/usr/bin/env bash
# Two Python CLI clients in parallel against MANAGER_INSTANCES=1.
# Each opens its own /s/new session, sends a unique magic word, captures
# claude's reply for ~50s, then disconnects (which auto-closes the session).
# Asserts each saw its OWN magic word and not the other's.
set -euo pipefail

PY=/home/llm/projects/llm-chat-py-cli/.venv/bin/python
CHAT=/home/llm/projects/llm-chat-py-cli/chat.py
M1=PINEAPPLE_ALPHA
M2=DAFFODIL_BRAVO

run_client() {
    local label="$1" magic="$2" out="$3"
    ( sleep 8; echo "reply with only the single word $magic"; sleep 50 ) \
      | "$PY" "$CHAT" 2>&1 \
      > "$out"
    echo "[$label] done -> $out ($(wc -c < "$out") bytes)"
}

OUT1=/tmp/cli-c1.log
OUT2=/tmp/cli-c2.log
rm -f "$OUT1" "$OUT2"

echo "spawning 2 parallel clients (one instance, /s/new each)..."
run_client C1 "$M1" "$OUT1" &
P1=$!
run_client C2 "$M2" "$OUT2" &
P2=$!

wait "$P1" "$P2"

# Count "answer marker `●` followed by magic word" in the raw stream — claude
# emits this when it's actually responding. Echo of the prompt only contains
# the magic word, never `●ANYWORD`. Grep handles multi-byte UTF-8 directly,
# so no ANSI strip is needed for this assertion.
C1_OWN=$(grep -c "● *$M1" "$OUT1" || true)
C1_FOREIGN=$(grep -c "$M2" "$OUT1" || true)
C2_OWN=$(grep -c "● *$M2" "$OUT2" || true)
C2_FOREIGN=$(grep -c "$M1" "$OUT2" || true)

echo
echo "=== summary ==="
echo "C1 magic=$M1 own=$C1_OWN foreign=$C1_FOREIGN"
echo "C2 magic=$M2 own=$C2_OWN foreign=$C2_FOREIGN"

if [ "$C1_OWN" -ge 1 ] && [ "$C1_FOREIGN" -eq 0 ] && [ "$C2_OWN" -ge 1 ] && [ "$C2_FOREIGN" -eq 0 ]; then
    echo "PASS"
    exit 0
else
    echo "FAIL"
    echo "--- C1 last 400 bytes (raw) ---"; tail -c 400 "$OUT1"
    echo "--- C2 last 400 bytes (raw) ---"; tail -c 400 "$OUT2"
    exit 1
fi
