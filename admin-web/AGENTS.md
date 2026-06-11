<!-- BEGIN:nextjs-agent-rules -->
# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` before writing any code. Heed deprecation notices.
<!-- END:nextjs-agent-rules -->

# Use shadcn/ui components — build custom only when none fits

This app is built on shadcn/ui (`components/ui/`). For ANY UI primitive, use the
existing shadcn component. Build a custom component ONLY when shadcn genuinely
has nothing for the job.

- **First** check `components/ui/` for the primitive (button, input, select,
  dropdown-menu, dialog, switch, checkbox, card, tooltip, table, tabs, sheet,
  popover, badge, etc.). If a needed shadcn component isn't installed yet, add
  it with the shadcn CLI rather than hand-rolling one.
- Do **not** hand-roll a styled `<button>`, `<div role="...">`, raw `<input>`,
  or a bespoke dropdown/badge/card when the shadcn equivalent exists. Compose
  shadcn primitives instead.
- A custom component is acceptable only for genuinely app-specific composition
  that isn't a primitive (e.g. a domain detail panel) — and even then, build it
  OUT OF shadcn primitives, not raw HTML.

**Why:** consistency (theming, dark mode, a11y, focus states all come from the
shadcn tokens), and raw HTML drifts from the design system and breaks in one
theme. A bespoke `<div>` with hand-written classes is the frontend version of
the "dirty fix" rule.

**How to apply:** before writing a styled element, ask "is this a shadcn
primitive?" If yes, import it. If you think none fits, say so explicitly and
name what you checked before going custom.
