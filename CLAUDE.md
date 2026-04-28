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
