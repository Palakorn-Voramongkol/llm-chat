# manager-4session

**4 sessions across 2 backends, no cross-talk.** Verifies the manager's
round-robin distribution, per-session routing, raw `/s/<sid>` data, AND
parsed `/qa/<sid>` events all work together.

## Setup

```bash
# from project root
setsid env MANAGER_STEALTH=1 MANAGER_INSTANCES=2 \
  ./manager/target/release/llm-chat-manager \
  > /tmp/manager.log 2>&1 < /dev/null &
disown
sleep 14
grep -E "OK|listening" /tmp/manager.log
# Expect:
#   backend ready instance_port=7878
#   backend ready instance_port=7879
#   manager listening addr=ws://127.0.0.1:7777
```

## Run

```bash
node testcases/e2e/cases/manager-4session/run.cjs
```

## PASS criteria

```
✓ PASS — 4 sessions, 4 distinct claude answers, no cross-talk
```

Each row of the per-session report:

```
[PASS] S1 sid=s1777… magic=MAGNOLIA1  /s/=Nb answerMarker=true occ=4 foreign=[] /qa/=Nev qaOk=true
```

means:
- `answerMarker=true` → claude printed `● MAGNOLIA1` (its answer marker followed
  by the magic word) in the raw stream — proves it actually answered, not just
  echoed the prompt
- `occ=4` → magic word appears in raw stream ≥2 times (echo + answer)
- `foreign=[]` → no other session's magic word leaked into this stream
- `/qa/=Nev qaOk=true` → at least one `qa-detected` event for THIS session
  contained the magic word in `answer`

Distribution check first (`distribution: { '7878': 2, '7879': 2 }`) confirms
the manager round-robins correctly.

## Cleanup

```bash
pkill -f llm-chat-manager
pkill -f xvfb-run
pkill -f Xvfb
pkill -f "release/llm-chat$"
```
