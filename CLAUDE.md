# Project rules for Claude

## No dirty fixes — root-cause everything

When something fails (build error, test failure, runtime error, hook
rejection, type error, anything), find the actual cause and fix that. Do not
apply a "dirty fix":

- do **not** skip the failing check (`--no-verify`, `# type: ignore`,
  `// eslint-disable-next-line` on the failing line)
- do **not** hardcode a value to step around the bug
- do **not** catch-and-ignore the error
- do **not** comment out the failing assertion

If a quick workaround is genuinely the only viable path right now (e.g.
unblocking an urgent demo), say so explicitly and flag the underlying issue
as remaining work. Do not pretend it's resolved.

**Why:** Quick fixes hide the real defect, accumulate as silent tech debt,
and usually surface again later in a worse form. Correctness over green-light
theater.

**How to apply:** Whenever a fix is tempting because it "makes the error go
away," pause and ask "what's actually broken here?" Trace the failure to its
source before editing. If the real fix is large, propose it explicitly rather
than shipping the band-aid; let the user decide.

## Use the source of truth — don't scrape-and-reconstruct

When you need data another program produces (its output, its state, its
structure), consume that program's **real, structured output** — not a
rendered/derived view of it that you then try to reverse-engineer.

- do **not** scrape a rendered terminal/TUI/HTML view and guess back the
  original structure with heuristics (width thresholds to "unwrap" lines,
  regexes to strip chrome, inserting blank lines to fake layout)
- do **not** stack pattern-matches to paper over a lossy capture
- **do** find the program's machine-readable mode (JSON/stream-json/`--print`,
  an API, an event stream) and use that as the single source of truth

**Why:** a rendered view has already lost information (wrapping, chrome,
truncation). Every heuristic to undo that is a guess that is fragile, never
exact, and accumulates as exactly the "dirty fixes" rule 1 forbids. Example
from this project: the worker scraped claude's xterm grid and reconstructed
answers with width/blank/chrome heuristics; the root-cause fix was to drive
claude with `--output-format stream-json` and read its actual answer text.

**How to apply:** when output "looks wrong," don't add another rule to the
parser — ask "is there a structured source I should be reading instead?" If the
current design forces reconstruction, treat that as the bug to fix.
