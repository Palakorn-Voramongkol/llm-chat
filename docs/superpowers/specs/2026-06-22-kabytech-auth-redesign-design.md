# kabytech auth UI redesign — split-screen brand panel

**Status:** designed 2026-06-22; not yet implemented. Visual-only redesign of the
kabytech auth pages (`/login`, `/invite`, `/accept`). **No auth-flow change.**

## Decision

A **two-column split-screen** layout replaces the centered card. All three pages
share `services/kabytech/frontend/components/Card.tsx` (`AuthCard`), so the
redesign lives entirely in that component — the pages are unchanged (the
`{title, subtitle, children}` API is preserved).

## Layout

```
desktop (>= md)                         mobile (< md)
┌────────────────┬────────────────┐     ┌────────────────────┐
│  ◆ kabytech    │  Sign in       │     │  ◆ kabytech (band) │
│                │  [ email     ] │     ├────────────────────┤
│  Your AI       │  [ password  ] │     │  Sign in           │
│  workspace,    │  [  Sign in  ] │     │  [ email         ] │
│  one login.    │                │     │  [ password      ] │
│  (gradient)    │                │     │  [   Sign in     ] │
└────────────────┴────────────────┘     └────────────────────┘
```

- **Left brand panel** (`hidden md:flex`, ~`md:w-1/2` / `lg:w-[45%]`): a rich
  indigo→violet gradient with a soft radial glow; a small inline SVG mark + the
  **kabytech** wordmark top-left; a large headline **"Your AI workspace, one
  login."** + one supporting sentence; a muted footer line. White text.
- **Right form column** (`flex-1`): white background, content vertically + center
  aligned, max-width ~`sm`; the page's `title` (h1), optional `subtitle`, then the
  `children` (form / states). Refined fields and a primary button.
- **Mobile** (`< md`): the left panel collapses to a slim top **band** (gradient
  strip with the mark + wordmark); the form column is full-width below it.

## Component changes (only `components/Card.tsx`)

- `AuthCard({title, subtitle, children})` → renders the split-screen shell + the
  mobile band. Same props; the three pages don't change.
- A tiny inline `BrandMark` SVG (a rounded gradient diamond) — no external asset.
- Refine the exports:
  - `inputCls` — larger padding, rounded-lg, a clear `focus:ring-2 focus:ring-indigo-500/40` + `focus:border-indigo-500`.
  - `btnCls` — full-width indigo primary, `font-semibold`, hover + disabled states.

## Error handling

Pure presentational change; no new failure modes. The form children (already in
each page) keep their own error rendering (`text-rose-600`).

## Testing

- **Existing** `app/{login,invite,accept}/page.test.tsx` keep passing (same
  placeholders, button roles, success states — the redesign doesn't touch them).
- **New** `components/Card.test.tsx`: `AuthCard` renders the `kabytech` wordmark,
  the headline, and the passed `title` + `children`.
- **Visual:** `pnpm run build`, then render and **screenshot `/login`, `/invite`,
  `/accept`** (desktop + a narrow viewport) and iterate on the look.

## Non-goals

No auth-flow / route / endpoint changes; no new pages; no dark mode; no external
images or icon libraries (one inline SVG only); headline copy is a one-line
constant, trivially editable.
