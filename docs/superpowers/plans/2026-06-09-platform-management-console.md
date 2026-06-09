# Platform-Management Console Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a professional VS Code-ribbon admin Console for the Zitadel-backed platform — Users, Roles & Grants, OIDC Applications, Project & Org settings, Dashboard, and a capability-gated Audit log — on the App=Project authorization model.

**Architecture:** A Next.js 16 / React 19 SPA under one `(dash)` route group fronted by the Rust axum admin-api (the only Zitadel caller); every area reuses three layers — shared shell + thin page, `Operator`-gated handler, `ZitadelClient` method. SA bumped to ORG_OWNER; Audit capability-gated.

**Tech Stack:** Next.js 16 (App Router), React 19, shadcn/ui, react-hook-form, zod, sonner, TanStack Table; Rust axum + reqwest; Zitadel v3.4.10 Management API; Playwright + vitest.

**Spec:** `docs/superpowers/specs/2026-06-09-platform-management-console-design.md`

---

## Phase 0: Foundation — shell, primitives, DataTable fix, ORG_OWNER bump

This phase builds the one shared shell every later area mounts under, the three missing shadcn primitives those areas consume, the prerequisite `emptyMessage` DataTable fix (so Roles/Apps/Audit don't all say "No users."), the Users-page refactor into the shell, and the deliberate `ORG_USER_MANAGER → ORG_OWNER` service-account bump (provisioner source + live update-member runbook). Everything in Phases 1–5 depends on this.

Grounded in: spec §4, §5, §6, §15; the real `components/ui/data-table.tsx` (hardcodes `No users.` at line 78), `app/(dash)/users/page.tsx` (owns its own `<main>`/`<h1>`/Sign-out today), `lib/{api,types}.ts` (`api.get<Me>("/api/me")`, `Me {userId,name,roles}`), `admin-api/src/{api/mod.rs,session.rs}` (the `/api/me` shape `{userId,name,roles}`), the approved mockup `docs/superpowers/specs/mockups/console-shell.html` (VS Code activity-bar ribbon, brand gradient `#5b53e8→#8b3df0`, side panel, colorful light theme), `deploy/compose/provisioner/{provision.py,test_provision.py}`, and `node_modules/next/dist/docs/01-app/.../route-groups.md` (a `(dash)` group adds **no** URL segment, so the shell layout wraps `/users` without changing its path).

**Files:**
- `admin-web/components/ui/data-table.tsx` — MODIFY: add optional `emptyMessage` prop, replace the hardcoded `"No users."` empty cell.
- `admin-web/__tests__/data-table.test.tsx` — CREATE: vitest proving the default + custom empty message render.
- `admin-web/components/ui/card.tsx` — CREATE: shadcn `card` primitive (Card/Header/Title/Description/Content/Footer).
- `admin-web/components/ui/switch.tsx` — CREATE: shadcn `switch` primitive over `radix-ui` Switch.
- `admin-web/components/ui/checkbox.tsx` — CREATE: shadcn `checkbox` primitive over `radix-ui` Checkbox.
- `admin-web/components/shell/nav.ts` — CREATE: the single typed `NAV` source-of-truth array (icon/label/href/match).
- `admin-web/components/shell/NavLink.tsx` — CREATE: active-aware `<Link>` using `usePathname`.
- `admin-web/components/shell/Sidebar.tsx` — CREATE: the VS Code activity-bar ribbon + brand, renders `NAV` via `NavLink`.
- `admin-web/components/shell/OperatorBadge.tsx` — CREATE: fetches `/api/me`, shows operator name/initials + sign-out.
- `admin-web/components/shell/Topbar.tsx` — CREATE: breadcrumb (from `NAV` + `usePathname`) + `OperatorBadge`.
- `admin-web/app/(dash)/layout.tsx` — CREATE: the only `'use client'` shell file; Sidebar + Topbar + `{children}` page slot.
- `admin-web/__tests__/shell-nav.test.tsx` — CREATE: vitest proving `NAV` shape + `NavLink` active highlight.
- `admin-web/app/(dash)/users/page.tsx` — MODIFY: drop the page-owned header/operator line/Sign-out (now the shell's job), pass `emptyMessage`.
- `admin-web/e2e/shell.spec.ts` — CREATE: Playwright check that the shell renders and the active nav highlights (`ADMIN_IT`-gated, mirrors `smoke.spec.ts`).
- `deploy/compose/provisioner/provision.py` — MODIFY: `ADMIN_SA_ROLE = "ORG_OWNER"`; add `update_admin_member` (live update-member PUT).
- `deploy/compose/provisioner/test_provision.py` — MODIFY: assert `ORG_OWNER`; add `update_admin_member` test.
- `deploy/compose/provisioner/README.md` — CREATE (or MODIFY if present): the runnable one-shot live `update-member` step.

---

### Task 0.1: Parameterize the DataTable empty-state (the prerequisite bug)

The shared table hardcodes `No users.` (line 78). Add an optional `emptyMessage` prop defaulting to a neutral string so Roles/Apps/Audit don't inherit "No users.".

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/data-table.test.tsx`:
```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ColumnDef } from "@tanstack/react-table";
import { DataTable } from "../components/ui/data-table";

type Row = { name: string };
const columns: ColumnDef<Row>[] = [{ accessorKey: "name", header: "Name" }];

describe("DataTable empty state", () => {
  it("shows a neutral default message when there are no rows", () => {
    render(<DataTable columns={columns} data={[]} />);
    expect(screen.getByText("No results.")).toBeInTheDocument();
  });

  it("shows a caller-supplied emptyMessage when there are no rows", () => {
    render(<DataTable columns={columns} data={[]} emptyMessage="No roles yet." />);
    expect(screen.getByText("No roles yet.")).toBeInTheDocument();
    expect(screen.queryByText("No users.")).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test data-table` → fails: `Unable to find an element with the text: No results.` (the cell still renders the hardcoded `No users.`).

- [ ] **Step 3: Implement** — in `admin-web/components/ui/data-table.tsx`, add `emptyMessage` to the props interface and the destructure, and render it. Replace the interface and signature, then the empty cell:
```tsx
interface DataTableProps<TData, TValue> {
  columns: ColumnDef<TData, TValue>[];
  data: TData[];
  filterColumn?: string;
  filterPlaceholder?: string;
  emptyMessage?: string;
}

export function DataTable<TData, TValue>({
  columns, data, filterColumn, filterPlaceholder, emptyMessage = "No results.",
}: DataTableProps<TData, TValue>) {
```
and replace the hardcoded empty cell body:
```tsx
                <TableCell colSpan={columns.length} className="h-24 text-center">
                  {emptyMessage}
                </TableCell>
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test data-table` → both assertions pass. Sanity: `cd admin-web; pnpm test` (existing `columns`/`api` suites still green).

- [ ] **Step 5: Commit**
```
git add admin-web/components/ui/data-table.tsx admin-web/__tests__/data-table.test.tsx
git commit -m "$(cat <<'EOF'
fix(admin-web): parameterize DataTable empty-state with emptyMessage

The shared table hardcoded "No users." — every reuse (Roles/Apps/Audit)
would inherit it. Add an emptyMessage prop (default "No results.").

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.2: Add the `card` shadcn primitive

Dashboard stat cards and the Project & Org settings cards need it. Mirror the repo's existing primitive style: `cn` from `@/lib/utils`, `data-slot`, plain `React.ComponentProps<"div">`.

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/ui-primitives.test.tsx`:
```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";

describe("Card primitive", () => {
  it("renders title and content", () => {
    render(
      <Card>
        <CardHeader><CardTitle>Total users</CardTitle></CardHeader>
        <CardContent>24</CardContent>
      </Card>,
    );
    expect(screen.getByText("Total users")).toBeInTheDocument();
    expect(screen.getByText("24")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test ui-primitives` → fails: `Failed to resolve import "../components/ui/card"`.

- [ ] **Step 3: Implement** — create `admin-web/components/ui/card.tsx`:
```tsx
import * as React from "react"

import { cn } from "@/lib/utils"

function Card({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card"
      className={cn(
        "bg-card text-card-foreground flex flex-col gap-6 rounded-xl border py-6 shadow-sm",
        className,
      )}
      {...props}
    />
  )
}

function CardHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-header"
      className={cn("flex flex-col gap-1.5 px-6", className)}
      {...props}
    />
  )
}

function CardTitle({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-title"
      className={cn("leading-none font-semibold", className)}
      {...props}
    />
  )
}

function CardDescription({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-description"
      className={cn("text-muted-foreground text-sm", className)}
      {...props}
    />
  )
}

function CardContent({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-content"
      className={cn("px-6", className)}
      {...props}
    />
  )
}

function CardFooter({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-footer"
      className={cn("flex items-center px-6", className)}
      {...props}
    />
  )
}

export { Card, CardHeader, CardTitle, CardDescription, CardContent, CardFooter }
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test ui-primitives` → passes.

- [ ] **Step 5: Commit**
```
git add admin-web/components/ui/card.tsx admin-web/__tests__/ui-primitives.test.tsx
git commit -m "$(cat <<'EOF'
feat(admin-web): add shadcn card primitive

Used by Dashboard stat cards and the Project & Org settings page.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.3: Add the `switch` shadcn primitive

Policy toggles (Project & Org) need it. The repo uses the single `radix-ui` package (`import { Switch } from "radix-ui"`), matching `button.tsx`/`badge.tsx`.

- [ ] **Step 1: Write the failing test** — append to `admin-web/__tests__/ui-primitives.test.tsx`:
```tsx
import { Switch } from "../components/ui/switch";

describe("Switch primitive", () => {
  it("renders an unchecked switch role", () => {
    render(<Switch aria-label="Skip MFA prompt" />);
    const s = screen.getByRole("switch", { name: "Skip MFA prompt" });
    expect(s).toBeInTheDocument();
    expect(s).toHaveAttribute("aria-checked", "false");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test ui-primitives` → fails: `Failed to resolve import "../components/ui/switch"`.

- [ ] **Step 3: Implement** — create `admin-web/components/ui/switch.tsx`:
```tsx
"use client"

import * as React from "react"
import { Switch as SwitchPrimitive } from "radix-ui"

import { cn } from "@/lib/utils"

function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        "peer data-[state=checked]:bg-primary data-[state=unchecked]:bg-input focus-visible:border-ring focus-visible:ring-ring/50 inline-flex h-5 w-9 shrink-0 items-center rounded-full border border-transparent shadow-xs transition-all outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className={cn(
          "bg-background pointer-events-none block size-4 rounded-full ring-0 transition-transform data-[state=checked]:translate-x-4 data-[state=unchecked]:translate-x-0",
        )}
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test ui-primitives` → passes.

- [ ] **Step 5: Commit**
```
git add admin-web/components/ui/switch.tsx admin-web/__tests__/ui-primitives.test.tsx
git commit -m "$(cat <<'EOF'
feat(admin-web): add shadcn switch primitive

Used by the Project & Org login/lockout policy toggles.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.4: Add the `checkbox` shadcn primitive

The Grants multiselect (Phase 1) needs it. Same `radix-ui` single-package convention; uses the `Check` lucide icon (already a dep).

- [ ] **Step 1: Write the failing test** — append to `admin-web/__tests__/ui-primitives.test.tsx`:
```tsx
import { Checkbox } from "../components/ui/checkbox";

describe("Checkbox primitive", () => {
  it("renders an unchecked checkbox role", () => {
    render(<Checkbox aria-label="chat.user" />);
    const c = screen.getByRole("checkbox", { name: "chat.user" });
    expect(c).toBeInTheDocument();
    expect(c).toHaveAttribute("aria-checked", "false");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test ui-primitives` → fails: `Failed to resolve import "../components/ui/checkbox"`.

- [ ] **Step 3: Implement** — create `admin-web/components/ui/checkbox.tsx`:
```tsx
"use client"

import * as React from "react"
import { Checkbox as CheckboxPrimitive } from "radix-ui"
import { Check } from "lucide-react"

import { cn } from "@/lib/utils"

function Checkbox({
  className,
  ...props
}: React.ComponentProps<typeof CheckboxPrimitive.Root>) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      className={cn(
        "peer border-input data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground data-[state=checked]:border-primary focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 aria-invalid:border-destructive size-4 shrink-0 rounded-[4px] border shadow-xs transition-shadow outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator
        data-slot="checkbox-indicator"
        className="flex items-center justify-center text-current transition-none"
      >
        <Check className="size-3.5" />
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  )
}

export { Checkbox }
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test ui-primitives` → passes.

- [ ] **Step 5: Commit**
```
git add admin-web/components/ui/checkbox.tsx admin-web/__tests__/ui-primitives.test.tsx
git commit -m "$(cat <<'EOF'
feat(admin-web): add shadcn checkbox primitive

Used by the per-user role-grant multiselect (Roles & Grants).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.5: The single typed `NAV` source-of-truth + active-aware `NavLink`

Spec §2: one typed `NAV` array (icon, label, href, match) consumed by the shell; adding an area = append one entry. `NavLink` highlights via `usePathname`. `match` is a path prefix so child routes (e.g. `/users/123`) still light the parent.

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/shell-nav.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { NAV, isActive } from "../components/shell/nav";
import { NavLink } from "../components/shell/NavLink";

vi.mock("next/navigation", () => ({ usePathname: () => "/users" }));

describe("NAV source of truth", () => {
  it("lists the six v1 areas with hrefs in order", () => {
    expect(NAV.map((n) => n.href)).toEqual([
      "/dashboard", "/users", "/roles", "/apps", "/settings", "/audit",
    ]);
    expect(NAV.map((n) => n.label)).toEqual([
      "Dashboard", "Users", "Roles", "Applications", "Project & Org", "Audit",
    ]);
  });

  it("isActive matches the area and its child routes by prefix", () => {
    expect(isActive("/users", "/users")).toBe(true);
    expect(isActive("/users/abc-123", "/users")).toBe(true);
    expect(isActive("/roles", "/users")).toBe(false);
  });
});

describe("NavLink", () => {
  it("marks the current route with aria-current=page", () => {
    render(<NavLink href="/users" match="/users" label="Users" />);
    expect(screen.getByRole("link", { name: "Users" }))
      .toHaveAttribute("aria-current", "page");
  });

  it("does not mark a non-current route", () => {
    render(<NavLink href="/roles" match="/roles" label="Roles" />);
    expect(screen.getByRole("link", { name: "Roles" }))
      .not.toHaveAttribute("aria-current");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test shell-nav` → fails: `Failed to resolve import "../components/shell/nav"`.

- [ ] **Step 3: Implement** — create `admin-web/components/shell/nav.ts`:
```ts
import {
  LayoutDashboard, Users, ShieldCheck, AppWindow, Building2, ScrollText,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  /** lucide icon component for the activity-bar ribbon. */
  icon: LucideIcon;
  /** human label (sidebar tooltip + topbar breadcrumb). */
  label: string;
  /** route this item navigates to. */
  href: string;
  /** path prefix that marks this item active (child routes included). */
  match: string;
}

// Single source of nav truth (spec §2): adding an area = append one entry
// + one page.tsx. Order is the build sequence from spec §15.
export const NAV: NavItem[] = [
  { icon: LayoutDashboard, label: "Dashboard", href: "/dashboard", match: "/dashboard" },
  { icon: Users, label: "Users", href: "/users", match: "/users" },
  { icon: ShieldCheck, label: "Roles", href: "/roles", match: "/roles" },
  { icon: AppWindow, label: "Applications", href: "/apps", match: "/apps" },
  { icon: Building2, label: "Project & Org", href: "/settings", match: "/settings" },
  { icon: ScrollText, label: "Audit", href: "/audit", match: "/audit" },
];

/** True when `pathname` is `match` or a child route of it. */
export function isActive(pathname: string, match: string): boolean {
  return pathname === match || pathname.startsWith(`${match}/`);
}
```
then create `admin-web/components/shell/NavLink.tsx`:
```tsx
"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { isActive } from "./nav";

interface NavLinkProps {
  href: string;
  match: string;
  label: string;
  icon?: LucideIcon;
}

export function NavLink({ href, match, label, icon: Icon }: NavLinkProps) {
  const pathname = usePathname();
  const active = isActive(pathname, match);
  return (
    <Link
      href={href}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors",
        active
          ? "bg-indigo-500/10 font-semibold text-indigo-600"
          : "text-foreground hover:bg-muted",
      )}
    >
      {Icon && (
        <Icon className={cn("size-4", active ? "text-indigo-600" : "text-muted-foreground")} />
      )}
      {label}
    </Link>
  );
}
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test shell-nav` → all four assertions pass.

- [ ] **Step 5: Commit**
```
git add admin-web/components/shell/nav.ts admin-web/components/shell/NavLink.tsx admin-web/__tests__/shell-nav.test.tsx
git commit -m "$(cat <<'EOF'
feat(admin-web): typed NAV source-of-truth + active-aware NavLink

One NAV array (icon/label/href/match) drives the whole shell; NavLink
highlights via usePathname and matches child routes by prefix.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.6: `OperatorBadge` — fetch `/api/me`, render name/initials + sign-out

Spec §4: fetches `/api/me` via `lib/api`; on 401 the api helper already full-page-redirects, so no auth handling here. Renders chrome immediately and fills in once `me` resolves (never blocks). `/api/me` returns `Me { userId, name, roles }` (verified in `lib/types.ts` + `admin-api/src/api/mod.rs::me`).

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/operator-badge.test.tsx`:
```tsx
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { OperatorBadge, initials } from "../components/shell/OperatorBadge";

afterEach(() => vi.restoreAllMocks());

describe("initials", () => {
  it("takes the first letter of up to two words, uppercased", () => {
    expect(initials("palakorn voramongkol")).toBe("PV");
    expect(initials("demo")).toBe("D");
    expect(initials("")).toBe("?");
  });
});

describe("OperatorBadge", () => {
  it("renders the operator name once /api/me resolves", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({ userId: "u1", name: "palakorn", roles: ["chat.admin"] }),
      headers: new Headers({ "content-type": "application/json" }),
    } as unknown as Response));
    render(<OperatorBadge />);
    expect(await screen.findByText("palakorn")).toBeInTheDocument();
  });

  it("renders a sign-out link to /logout", () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({ userId: "u1", name: "x", roles: [] }),
      headers: new Headers({ "content-type": "application/json" }),
    } as unknown as Response));
    render(<OperatorBadge />);
    expect(screen.getByRole("link", { name: /sign out/i }))
      .toHaveAttribute("href", "/logout");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test operator-badge` → fails: `Failed to resolve import "../components/shell/OperatorBadge"`.

- [ ] **Step 3: Implement** — create `admin-web/components/shell/OperatorBadge.tsx`:
```tsx
"use client";
import { useEffect, useState } from "react";
import { LogOut } from "lucide-react";
import { api } from "@/lib/api";
import type { Me } from "@/lib/types";

/** First letter of up to two words, uppercased; "?" when empty. */
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  return parts.slice(0, 2).map((p) => p[0]!.toUpperCase()).join("");
}

export function OperatorBadge() {
  const [me, setMe] = useState<Me | null>(null);

  useEffect(() => {
    // 401 inside lib/api full-page-redirects to /login; swallow here (spec §4).
    api.get<Me>("/api/me").then(setMe).catch(() => {});
  }, []);

  return (
    <div className="flex items-center gap-3">
      <span className="flex items-center gap-2 text-sm font-semibold">
        <span className="flex size-8 items-center justify-center rounded-full bg-gradient-to-br from-indigo-500 to-violet-500 text-xs font-bold text-white">
          {initials(me?.name ?? "")}
        </span>
        {me?.name ?? "—"}
      </span>
      <a
        href="/logout"
        className="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 text-sm"
      >
        <LogOut className="size-4" />
        <span>Sign out</span>
      </a>
    </div>
  );
}
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test operator-badge` → all assertions pass.

- [ ] **Step 5: Commit**
```
git add admin-web/components/shell/OperatorBadge.tsx admin-web/__tests__/operator-badge.test.tsx
git commit -m "$(cat <<'EOF'
feat(admin-web): OperatorBadge fetches /api/me, shows operator + sign-out

Renders chrome immediately; 401 is handled by lib/api's full-page redirect.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.7: `Sidebar` (activity-bar ribbon) + `Topbar` (breadcrumb)

Matches the approved mockup: a 60px brand-gradient activity-bar ribbon (`#5b53e8 → #8b3df0`) with the lucide icons from `NAV`, plus a white topbar with a `Console / <Area>` breadcrumb derived from `NAV` + `usePathname`, and the `OperatorBadge` on the right. Sidebar icons use native `title` tooltips (no extra dep) and reuse `NavLink`'s active logic via `isActive`.

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/shell-chrome.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { Sidebar } from "../components/shell/Sidebar";
import { Topbar } from "../components/shell/Topbar";

vi.mock("next/navigation", () => ({ usePathname: () => "/users" }));
vi.mock("../lib/api", () => ({
  api: { get: () => Promise.resolve({ userId: "u1", name: "x", roles: [] }) },
  ApiError: class {},
}));

describe("Sidebar", () => {
  it("renders a labelled link for every NAV area", () => {
    render(<Sidebar />);
    for (const label of ["Dashboard", "Users", "Roles", "Applications", "Project & Org", "Audit"]) {
      expect(screen.getByRole("link", { name: new RegExp(label, "i") })).toBeInTheDocument();
    }
  });

  it("marks the current area active", () => {
    render(<Sidebar />);
    expect(screen.getByRole("link", { name: /Users/i })).toHaveAttribute("aria-current", "page");
  });
});

describe("Topbar", () => {
  it("shows the current area in the breadcrumb", () => {
    render(<Topbar />);
    expect(screen.getByText("Users")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test shell-chrome` → fails: `Failed to resolve import "../components/shell/Sidebar"`.

- [ ] **Step 3: Implement** — create `admin-web/components/shell/Sidebar.tsx`:
```tsx
"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Boxes } from "lucide-react";
import { cn } from "@/lib/utils";
import { NAV, isActive } from "./nav";

export function Sidebar() {
  const pathname = usePathname();
  return (
    <nav
      aria-label="Primary"
      className="flex w-[60px] shrink-0 flex-col items-center gap-1.5 bg-gradient-to-b from-[#5b53e8] to-[#8b3df0] py-3 shadow-[2px_0_16px_rgba(91,83,232,0.22)]"
    >
      <div className="mb-3 flex size-[34px] items-center justify-center rounded-[10px] bg-white/15 text-white">
        <Boxes className="size-5" />
      </div>
      {NAV.map((item) => {
        const active = isActive(pathname, item.match);
        const Icon = item.icon;
        return (
          <Link
            key={item.href}
            href={item.href}
            title={item.label}
            aria-label={item.label}
            aria-current={active ? "page" : undefined}
            className={cn(
              "relative flex size-11 items-center justify-center rounded-xl transition-colors",
              active ? "bg-white/20 text-white" : "text-white/70 hover:bg-white/10 hover:text-white",
            )}
          >
            <Icon className="size-[22px]" />
            {active && (
              <span className="absolute -left-3 top-2.5 bottom-2.5 w-[3px] rounded-r-[3px] bg-white" />
            )}
          </Link>
        );
      })}
    </nav>
  );
}
```
then create `admin-web/components/shell/Topbar.tsx`:
```tsx
"use client";
import { usePathname } from "next/navigation";
import { NAV, isActive } from "./nav";
import { OperatorBadge } from "./OperatorBadge";

export function Topbar() {
  const pathname = usePathname();
  const current = NAV.find((n) => isActive(pathname, n.match));
  return (
    <header className="bg-card flex h-15 items-center gap-3 border-b px-6 py-3">
      <span className="text-muted-foreground text-sm">
        Console / <span className="text-foreground font-medium">{current?.label ?? "Home"}</span>
      </span>
      <span className="flex-1" />
      <OperatorBadge />
    </header>
  );
}
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test shell-chrome` → all assertions pass.

- [ ] **Step 5: Commit**
```
git add admin-web/components/shell/Sidebar.tsx admin-web/components/shell/Topbar.tsx admin-web/__tests__/shell-chrome.test.tsx
git commit -m "$(cat <<'EOF'
feat(admin-web): VS Code activity-bar Sidebar + breadcrumb Topbar

Brand-gradient ribbon driven by NAV; topbar breadcrumb + OperatorBadge.
Matches the approved console-shell.html mockup.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.8: The `(dash)` shell layout (the only shell file)

Spec §4: one `'use client'` shell at `app/(dash)/layout.tsx` rendering Sidebar + Topbar + `{children}` as the page slot. Per `route-groups.md` the `(dash)` group adds **no** URL segment, so this layout wraps `/users` without changing its path. Next 16 async-`params` does not apply here — `layout.tsx` takes only `children` (no `params` destructured), so there is nothing to await.

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/dash-layout.test.tsx`:
```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import DashLayout from "../app/(dash)/layout";

vi.mock("next/navigation", () => ({ usePathname: () => "/users" }));
vi.mock("../lib/api", () => ({
  api: { get: () => Promise.resolve({ userId: "u1", name: "x", roles: [] }) },
  ApiError: class {},
}));

describe("(dash) shell layout", () => {
  it("renders the sidebar, topbar, and the page slot children", () => {
    render(<DashLayout><div data-testid="page-slot">PAGE</div></DashLayout>);
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByText("Console /")).toBeInTheDocument();
    expect(screen.getByTestId("page-slot")).toHaveTextContent("PAGE");
  });
});
```
(Note: the breadcrumb renders the literal text `Console /` plus the area name as a sibling, so `getByText("Console /")` matches the prefix node.)

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test dash-layout` → fails: `Failed to resolve import "../app/(dash)/layout"`.

- [ ] **Step 3: Implement** — create `admin-web/app/(dash)/layout.tsx`:
```tsx
"use client";
import type { ReactNode } from "react";
import { Sidebar } from "@/components/shell/Sidebar";
import { Topbar } from "@/components/shell/Topbar";

// The ONLY shell file (spec §4). The (dash) route group adds no URL segment
// (Next 16 route-groups), so /users etc. render inside this chrome unchanged.
export default function DashLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <Topbar />
        <main className="flex-1 overflow-auto">{children}</main>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test dash-layout` → passes. Then build-sanity: `cd admin-web; pnpm build` succeeds (route group compiles; `/users` still resolves).

- [ ] **Step 5: Commit**
```
git add "admin-web/app/(dash)/layout.tsx" admin-web/__tests__/dash-layout.test.tsx
git commit -m "$(cat <<'EOF'
feat(admin-web): (dash) shell layout mounting Sidebar + Topbar

Single 'use client' shell; route group adds no URL segment so /users
renders inside the chrome unchanged.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.9: Refactor Users page into the shell

Spec §6: strip the page-owned `<main className="container">`, the `<h1>` + operator line, and the Sign-out button (now the shell's job). What remains is the canonical list + create + lifecycle + confirm page. Add a per-page title region (lighter than before) and pass `emptyMessage="No users."` to keep the Users empty-state wording while every other table now gets its own. The `me` fetch and the operator line move out.

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/users-page.test.tsx`:
```tsx
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import UsersPage from "../app/(dash)/users/page";

vi.mock("next/navigation", () => ({ usePathname: () => "/users" }));

afterEach(() => vi.restoreAllMocks());

function stubFetch(body: unknown) {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
    ok: true, status: 200, json: async () => body,
    headers: new Headers({ "content-type": "application/json" }),
  } as unknown as Response));
}

describe("Users page (shell-refactored)", () => {
  it("renders the Users heading and the empty-state with 'No users.'", async () => {
    stubFetch({ result: [] });
    render(<UsersPage />);
    expect(await screen.findByRole("heading", { name: "Users" })).toBeInTheDocument();
    expect(await screen.findByText("No users.")).toBeInTheDocument();
  });

  it("no longer renders its own Sign out control (the shell owns it)", async () => {
    stubFetch({ result: [] });
    render(<UsersPage />);
    expect(screen.queryByRole("link", { name: /sign out/i })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `cd admin-web; pnpm test users-page` → fails: the page still renders a `Sign out` link (and the old `Signed in as` line), so the second assertion fails.

- [ ] **Step 3: Implement** — replace `admin-web/app/(dash)/users/page.tsx` with the shell-refactored version (drops the `me` state, the operator line, the Sign-out button, and the `container`/`<main>`; keeps load/lifecycle/delete intact; passes `emptyMessage`):
```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { buildColumns, type Lifecycle } from "@/components/users/columns";
import { CreateUserDialog } from "@/components/users/create-user-dialog";
import { EditUserDialog } from "@/components/users/edit-user-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { api, ApiError } from "@/lib/api";
import type { User, UserList } from "@/lib/types";

export default function UsersPage() {
  const [users, setUsers] = useState<User[]>([]);
  const [editTarget, setEditTarget] = useState<User | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.get<UserList>("/api/users");
      setUsers(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load users");
      }
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  async function onLifecycle(u: User, action: Lifecycle) {
    try {
      await api.post(`/api/users/${u.id}/${action}`);
      toast.success(`${action} ok`);
      load();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : `${action} failed`);
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      await api.del(`/api/users/${deleteTarget.id}`);
      toast.success("User deleted");
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    } finally {
      setDeleteTarget(null);
      load();
    }
  }

  const columns = buildColumns({
    onEdit: setEditTarget,
    onDelete: setDeleteTarget,
    onLifecycle,
  });

  return (
    <div className="space-y-4 px-6 py-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold">Users</h1>
          <p className="text-muted-foreground text-sm">
            People and machine accounts across every app on the platform.
          </p>
        </div>
        <CreateUserDialog onCreated={load} />
      </div>
      <DataTable columns={columns} data={users}
        filterColumn="userName" filterPlaceholder="Filter by username..."
        emptyMessage="No users." />
      <EditUserDialog user={editTarget} open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)} onSaved={load} />
      <ConfirmDialog open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete user?"
        description="This is irreversible and removes the user and any machine keys. Already-issued tokens stay valid until their TTL expires."
        confirmLabel="Delete" onConfirm={confirmDelete} />
    </div>
  );
}
```

- [ ] **Step 4: Run it, expect PASS** — `cd admin-web; pnpm test users-page` → both assertions pass. Regression: `cd admin-web; pnpm test` (all suites green). The existing `e2e/smoke.spec.ts` still asserts `heading { name: "Users" }`, which this preserves.

- [ ] **Step 5: Commit**
```
git add "admin-web/app/(dash)/users/page.tsx" admin-web/__tests__/users-page.test.tsx
git commit -m "$(cat <<'EOF'
refactor(admin-web): move Users page into the shell

Drop the page-owned <main>/<h1>/operator-line/Sign-out (now the shell's
job) and pass emptyMessage="No users." Page is now the canonical
list+create+lifecycle+confirm shape every area mirrors.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.10: Playwright — shell renders + nav highlights

Spec §13: extend the e2e suite. Mirror `smoke.spec.ts` exactly (the `ADMIN_IT` gate + the real Zitadel login + 2FA-skip helper). After landing on `/users`, assert the activity-bar ribbon rendered all six areas and the Users item is the active one (`aria-current="page"`), proving the shell + `usePathname` highlight work end-to-end.

- [ ] **Step 1: Write the failing test** — create `admin-web/e2e/shell.spec.ts`:
```ts
import { test, expect } from "@playwright/test";

const FULL = process.env.ADMIN_IT === "1";

test.describe("console shell", () => {
  test.skip(!FULL, "requires running stack: set ADMIN_IT=1 + a chat.admin session");

  test("renders the activity-bar nav and highlights the active area", async ({ page }) => {
    // Real login against Zitadel v3 (operator with chat.admin), same as smoke.spec.ts.
    await page.goto("/login");
    await page.locator('input[name="loginName"]').fill(process.env.ADMIN_IT_USER!);
    await page.getByRole("button", { name: /next|continue/i }).click();
    await page.locator('input[name="password"]').fill(process.env.ADMIN_IT_PASS!);
    await page.getByRole("button", { name: /next|continue|sign in/i }).click();

    const skip2fa = page.getByRole("button", { name: /skip/i });
    await skip2fa
      .waitFor({ state: "visible", timeout: 8000 })
      .then(() => skip2fa.click())
      .catch(() => {});

    await page.waitForURL(/\/users/);

    // The activity-bar ribbon renders every NAV area.
    const nav = page.getByRole("navigation", { name: "Primary" });
    for (const label of ["Dashboard", "Users", "Roles", "Applications", "Project & Org", "Audit"]) {
      await expect(nav.getByRole("link", { name: new RegExp(label, "i") })).toBeVisible();
    }

    // usePathname marks Users active; the others are not current.
    await expect(nav.getByRole("link", { name: /Users/i })).toHaveAttribute("aria-current", "page");
    await expect(nav.getByRole("link", { name: /Roles/i })).not.toHaveAttribute("aria-current", "page");

    // Topbar breadcrumb + operator badge are present (shell, not page).
    await expect(page.getByText("Console /")).toBeVisible();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL (gated)** — without the stack: `cd admin-web; pnpm e2e shell` → the test reports **skipped** (`requires running stack`), which is the expected pre-stack state (matches how `smoke.spec.ts`'s authenticated block is gated). With the stack up: `cd admin-web; $env:ADMIN_IT="1"; $env:ADMIN_IT_USER="demo"; $env:ADMIN_IT_PASS="GogoPure0811!"; pnpm e2e shell` → fails on the nav assertions until the shell is wired (it is, from Tasks 0.5–0.8), confirming the harness exercises the real chrome.

- [ ] **Step 3: Implement** — no new app code; the shell from Tasks 0.5–0.8 already satisfies it. (If the breadcrumb assertion needs adjusting to the rendered text, fix the test, not the shell.)

- [ ] **Step 4: Run it, expect PASS** — with the full stack: `cd admin-web; $env:ADMIN_IT="1"; $env:ADMIN_IT_USER="demo"; $env:ADMIN_IT_PASS="GogoPure0811!"; pnpm e2e shell` → passes (all six links visible, Users active, breadcrumb shown).

- [ ] **Step 5: Commit**
```
git add admin-web/e2e/shell.spec.ts
git commit -m "$(cat <<'EOF'
test(admin-web): e2e shell renders nav + highlights active area

ADMIN_IT-gated Playwright: after login lands on /users, asserts all six
activity-bar areas render and Users carries aria-current=page.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.11: ORG_OWNER bump in the provisioner (source of truth)

Spec §3/§5: the runtime SA is `ORG_USER_MANAGER`, which cannot write policies/project/roles/apps — those 403 until the bump. Change `ADMIN_SA_ROLE` to `ORG_OWNER` so a clean re-provision grants full org ownership. This is the deliberate, surfaced privilege bump. Update the existing assertion test first.

- [ ] **Step 1: Update the failing test** — in `deploy/compose/provisioner/test_provision.py`, change the existing `test_assign_admin_member_posts_org_user_manager` assertion (currently `assert b["roles"] == ["ORG_USER_MANAGER"]`, line ~269) and rename it:
```python
def test_assign_admin_member_posts_org_owner():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.assign_admin_member("boot-tok", {"h": "1"}, "sa-123")
    assert captured["url"].endswith("/management/v1/orgs/me/members")
    b = captured["body"]
    assert b["userId"] == "sa-123"
    assert b["roles"] == ["ORG_OWNER"]
```

- [ ] **Step 2: Run it, expect FAIL** — `cd deploy/compose/provisioner; python -m pytest test_provision.py -k org_owner -q` → fails: `assert ['ORG_USER_MANAGER'] == ['ORG_OWNER']` (the constant is still `ORG_USER_MANAGER`).

- [ ] **Step 3: Implement** — in `deploy/compose/provisioner/provision.py`, change the constant (line 56):
```python
ADMIN_SA_ROLE = "ORG_OWNER"  # full org ownership; bumped from ORG_USER_MANAGER per spec §3/§5
```
and update the `assign_admin_member` docstring's stale "least privilege" note to reflect the deliberate bump:
```python
def assign_admin_member(token: str, headers: dict, sa_user_id: str) -> None:
    """Grant the admin SA org ownership (spec §3/§5). MUST be called with the
    BOOTSTRAP IAM_OWNER token (needs org.member.write) — NOT the new SA itself.
    orgs/me resolves the org from the calling token / x-zitadel-orgid.
    Idempotent: 409 == already a member. ORG_OWNER is required so the admin-api
    can write policies, the project, roles, and apps (ORG_USER_MANAGER 403'd)."""
```

- [ ] **Step 4: Run it, expect PASS** — `cd deploy/compose/provisioner; python -m pytest test_provision.py -q` → all tests pass (the renamed assertion + the unchanged `main()` integration test, which mocks `assign_admin_member`).

- [ ] **Step 5: Commit**
```
git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py
git commit -m "$(cat <<'EOF'
feat(provisioner): bump admin SA role ORG_USER_MANAGER -> ORG_OWNER

ORG_USER_MANAGER cannot write policies/project/roles/apps (403). The
admin Console needs full org ownership; mitigated by the SA key never
leaving the BFF + every action gated behind a chat.admin operator (§3).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.12: Live one-shot `update-member` (already-provisioned instance) + runbook

Spec §5: a bare re-run of the provisioner **no-ops** on the existing member (`assign_admin_member` POST → 409 == already a member, treated as success). So the running instance keeps `ORG_USER_MANAGER`. To bump it live you must call `PUT /management/v1/orgs/me/members/{saUserId}` with `roles=[ORG_OWNER]` using the bootstrap IAM_OWNER key. Add a tested `update_admin_member` helper and a runnable doc step.

- [ ] **Step 1: Write the failing test** — append to `deploy/compose/provisioner/test_provision.py`:
```python
def test_update_admin_member_puts_org_owner_to_member_path():
    captured = {}

    def fake_rwr(method, url, *, headers=None, json_body=None, **kw):
        captured["method"] = method
        captured["url"] = url
        captured["body"] = json_body
        return _FakeResp(200, {"details": {}})

    with mock.patch.object(provision, "request_with_retry", fake_rwr):
        provision.update_admin_member("boot-tok", {"h": "1"}, "sa-123")
    assert captured["method"] == "PUT"
    assert captured["url"].endswith("/management/v1/orgs/me/members/sa-123")
    assert captured["body"]["roles"] == ["ORG_OWNER"]


def test_update_admin_member_raises_on_hard_error():
    with mock.patch.object(provision, "request_with_retry",
                           lambda *a, **k: _FakeResp(400)):
        with pytest.raises(RuntimeError):
            provision.update_admin_member("boot-tok", {}, "sa-123")
```

- [ ] **Step 2: Run it, expect FAIL** — `cd deploy/compose/provisioner; python -m pytest test_provision.py -k update_admin_member -q` → fails: `AttributeError: module 'provision' has no attribute 'update_admin_member'`.

- [ ] **Step 3: Implement** — in `deploy/compose/provisioner/provision.py`, add the helper directly below `assign_admin_member`:
```python
def update_admin_member(token: str, headers: dict, sa_user_id: str) -> None:
    """Live-bump an EXISTING org member's roles to [ORG_OWNER] (spec §5).

    A bare provisioner re-run no-ops (assign_admin_member POSTs and Zitadel
    409s == already a member), so the already-provisioned instance keeps its
    old ORG_USER_MANAGER role. This PUT *updates* the existing member. MUST be
    called with the BOOTSTRAP IAM_OWNER token (org.member.write)."""
    resp = request_with_retry(
        "PUT", f"{ISSUER}/management/v1/orgs/me/members/{sa_user_id}",
        headers=headers, json_body={"roles": [ADMIN_SA_ROLE]},
    )
    if not is_success(resp.status_code):
        resp.raise_for_status()
```
This is a helper for the documented one-shot below; `main()` (clean-boot) keeps using `assign_admin_member`.

- [ ] **Step 4: Run it, expect PASS** — `cd deploy/compose/provisioner; python -m pytest test_provision.py -q` → all pass.

- [ ] **Step 5: Commit**
```
git add deploy/compose/provisioner/provision.py deploy/compose/provisioner/test_provision.py
git commit -m "$(cat <<'EOF'
feat(provisioner): update_admin_member PUT for live ORG_OWNER bump

A re-run no-ops on the existing member (409); the already-provisioned
instance needs a PUT /orgs/me/members/{id} roles=[ORG_OWNER] to update.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 0.13: Document the runnable live `update-member` step

Spec §5: capture the one-shot live bump as a runnable runbook so future operators don't re-run the provisioner and wonder why the role didn't change. Add it to the provisioner README (create the section if the file lacks it).

- [ ] **Step 1: Write the doc** — add to `deploy/compose/provisioner/README.md` (create the file with this content if it does not exist; otherwise append the section):
````markdown
## Live ORG_OWNER bump (already-provisioned instance)

The provisioner grants the admin service account `ORG_OWNER` on a **clean**
boot (`assign_admin_member`). A bare re-run **no-ops** on an existing member
(Zitadel returns 409 == already a member, treated as success), so an instance
provisioned before the bump keeps its old `ORG_USER_MANAGER` role and the
admin Console's policy/project/role/app writes will 403.

To bump the live instance without re-provisioning, run the one-shot
`update-member` (`PUT /management/v1/orgs/me/members/{saUserId}`) with the
**bootstrap IAM_OWNER** key (it needs `org.member.write`; the runtime SA does
not). From the provisioner directory:

```bash
python - <<'PY'
import json, provision
boot = provision.load_admin_key()                       # bootstrap IAM_OWNER key
token = provision.mint_management_token(boot)
org_id = provision.fetch_org_id(token)
headers = provision.mgmt_headers(token, org_id)
sa_user_id = open(f"{provision.SECRETS_DIR}/admin_api_user_id").read().strip()
provision.update_admin_member(token, headers, sa_user_id)
print(f"bumped {sa_user_id} -> {provision.ADMIN_SA_ROLE}")
PY
```

Verify (expect `ORG_OWNER` in the member's roles):

```bash
curl -s -X POST "$PROVISION_ISSUER/management/v1/orgs/me/members/_search" \
  -H "Authorization: Bearer $BOOT_TOKEN" -H "Content-Type: application/json" \
  -d '{}' | python -c 'import sys,json; \
[print(m["userId"], m["roles"]) for m in json.load(sys.stdin).get("result", [])]'
```
````

- [ ] **Step 2: Verify it runs (dry sanity)** — confirm the snippet references only real symbols: `cd deploy/compose/provisioner; python -c "import provision; print(all(hasattr(provision, n) for n in ['load_admin_key','mint_management_token','fetch_org_id','mgmt_headers','update_admin_member','SECRETS_DIR','ADMIN_SA_ROLE']))"` → prints `True`.

- [ ] **Step 3: Implement** — none beyond the doc (the helper exists from Task 0.12).

- [ ] **Step 4: Re-run the provisioner test suite** — `cd deploy/compose/provisioner; python -m pytest -q` → all pass (doc change does not affect tests; confirms nothing regressed).

- [ ] **Step 5: Commit**
```
git add deploy/compose/provisioner/README.md
git commit -m "$(cat <<'EOF'
docs(provisioner): runbook for the live ORG_OWNER update-member bump

A re-run no-ops on existing members; document the one-shot PUT with the
bootstrap IAM_OWNER key + a verify query.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

**Phase 0 exit criteria (all verifiable):**
- `cd admin-web; pnpm test` → all vitest suites green (data-table, ui-primitives, shell-nav, operator-badge, shell-chrome, dash-layout, users-page, plus the pre-existing api/columns/next-config suites).
- `cd admin-web; pnpm build` → compiles; `/users` resolves inside the `(dash)` shell.
- `cd deploy/compose/provisioner; python -m pytest -q` → all green with `ADMIN_SA_ROLE == "ORG_OWNER"`.
- With the stack up: `cd admin-web; $env:ADMIN_IT="1"; pnpm e2e shell` → shell renders, Users highlighted.
- The live-bump runbook is documented and references only real `provision` symbols.

All later phases (Roles/Apps/Settings/Dashboard/Audit) now mount thin pages under this shell, reuse the `emptyMessage` DataTable + the three new primitives, and rely on the `ORG_OWNER` SA for their policy/project/role/app writes.

## Phase 1: Roles & Grants

Implements spec §7. The grant set-math primitives (`add_grant`/`set_grant_roles`/`remove_grant`) and the per-user grant routes already exist in `admin-api/src/zitadel/grants.rs` + `admin-api/src/api/mod.rs`; this phase adds the **role** lifecycle (create/delete/holders) on the backend, a **Roles page** on the frontend, and a per-user **Grants UI** that drives the existing grant endpoints through the one-grant-per-project POST/PUT/DELETE branch. It also adds the NAV entry for Roles.

**Verified Zitadel paths (grounded in `docs/superpowers/specs/2026-06-07-zitadel-admin-api-reference.md` §3.3/§3.4 and `deploy/compose/provisioner/provision.py:create_admin_role`):**
- Create role: `POST /management/v1/projects/{pid}/roles` body `{roleKey, displayName, group}` → `{details}` (no role id; `roleKey` IS the id).
- Delete role: `DELETE /management/v1/projects/{pid}/roles/{roleKey}` (cascades — strips the role from every grant).
- Holders: `POST /management/v1/users/grants/_search` body `{queries:[{projectIdQuery:{projectId}},{roleKeyQuery:{roleKey}}]}` → items `{id, userId, roleKeys[], displayName, ...}` (queries AND-combine).

**Prerequisite (Phase 0):** assumes `app/(dash)/layout.tsx` shell with a typed `NAV` array, the `emptyMessage` prop on `components/ui/data-table.tsx`, and the `checkbox` shadcn primitive all exist. If `checkbox` is absent, Task 1.6 Step 0 adds it. The grant set-math math (`roles_without`) and routes already exist — this phase does NOT re-add them.

**Files:**
- `admin-api/src/zitadel/grants.rs` — MODIFY: add `create_role`, `delete_role`, `list_role_holders` methods + pure unit tests for the holders query body.
- `admin-api/src/api/mod.rs` — MODIFY: add `POST /api/roles`, `DELETE /api/roles/{roleKey}`, `GET /api/roles/{roleKey}/holders` routes + handlers + a camelCase contract test for `CreateRole`.
- `admin-web/lib/types.ts` — MODIFY: add `RoleList`, `RoleHolder`, `RoleHolderList`, `GrantList` types.
- `admin-web/app/(dash)/roles/page.tsx` — CREATE: the Roles list page (DataTable + CreateRoleDialog + HoldersDialog + cascade-warning ConfirmDialog).
- `admin-web/components/roles/columns.tsx` — CREATE: role table columns + actions dropdown (View holders / Delete-cascade).
- `admin-web/components/roles/create-role-dialog.tsx` — CREATE: react-hook-form + zod create-role dialog.
- `admin-web/components/roles/holders-dialog.tsx` — CREATE: dialog listing who holds a role.
- `admin-web/components/users/grants-dialog.tsx` — CREATE: the per-user Grants UI (checkbox multiselect of `list_roles`, drives the POST/PUT/DELETE branch).
- `admin-web/components/users/columns.tsx` — MODIFY: add an "Access (grants)" dropdown item + `onGrants` handler.
- `admin-web/app/(dash)/users/page.tsx` — MODIFY: wire the GrantsDialog + `onGrants`.
- `admin-web/components/shell/Sidebar.tsx` (or wherever Phase 0's `NAV` lives) — MODIFY: append the Roles NAV entry (`ShieldCheck`, `/roles`).
- `admin-web/e2e/smoke.spec.ts` — MODIFY: add a Roles create/delete test and a grant assign/revoke round-trip.

---

### Task 1.1: Backend — `create_role` ZitadelClient method

- [ ] **Step 1: Write the failing test.** Append to the `#[cfg(test)] mod tests` block in `admin-api/src/zitadel/grants.rs` a pure test asserting the holders search body shape (the method bodies hit the network, so we test the pure body builder we will extract). Add this test:
```rust
    #[test]
    fn holders_query_filters_by_project_and_role_anded() {
        let body = super::holders_search_body("p1", "chat.admin");
        let queries = body.get("queries").and_then(Value::as_array).unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0]["projectIdQuery"]["projectId"], "p1");
        assert_eq!(queries[1]["roleKeyQuery"]["roleKey"], "chat.admin");
    }
```
- [ ] **Step 2: Run it, expect FAIL.** Run `cargo test -p admin-api --lib zitadel::grants` from `admin-api/`. Expect: `error[E0425]: cannot find function 'holders_search_body' in module 'super'`.
- [ ] **Step 3: Implement.** Add the pure body builder + the three methods to the `impl ZitadelClient` block in `admin-api/src/zitadel/grants.rs` (above the closing `}` of the impl, after `remove_grant`). First add the pure free function near `roles_without` (top of file, after it):
```rust
/// Pure: the grants/_search body that finds every holder of `role` in
/// `pid`. Two queries AND-combine (reference §3.4): project + role.
pub fn holders_search_body(project_id: &str, role_key: &str) -> Value {
    json!({ "queries": [
        { "projectIdQuery": { "projectId": project_id } },
        { "roleKeyQuery": { "roleKey": role_key } },
    ] })
}
```
Then inside `impl ZitadelClient` add:
```rust
    /// Create a project role (§3.3): POST /projects/{pid}/roles {roleKey,
    /// displayName, group}. `roleKey` is the unique id (no separate id returned).
    pub async fn create_role(&self, role_key: &str, display_name: &str, group: &str) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/roles", self.cfg.issuer, pid);
        let body = json!({ "roleKey": role_key, "displayName": display_name, "group": group });
        self.post_json(&url, &body).await.map(|_| ())
    }

    /// Delete a project role (§3.3): DELETE /projects/{pid}/roles/{roleKey}.
    /// CASCADES — strips the role from every user grant (design §7 warning).
    pub async fn delete_role(&self, role_key: &str) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/roles/{}", self.cfg.issuer, pid, role_key);
        self.delete(&url).await.map(|_| ())
    }

    /// List holders of a role (§3.4): POST /users/grants/_search filtered by
    /// project + roleKey. Items carry {id, userId, roleKeys[], displayName,...}.
    pub async fn list_role_holders(&self, role_key: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/users/grants/_search", self.cfg.issuer);
        let body = holders_search_body(&self.cfg.project_id, role_key);
        let v = self.post_json(&url, &body).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }
```
- [ ] **Step 4: Run it, expect PASS.** Run `cargo test -p admin-api --lib zitadel::grants` from `admin-api/`. Expect `test result: ok` with `holders_query_filters_by_project_and_role_anded` passing alongside the existing `roles_without_*` tests.
- [ ] **Step 5: Commit.** `git add admin-api/src/zitadel/grants.rs && git commit -m "feat(admin-api): create_role/delete_role/list_role_holders zitadel methods"` (append the Co-Authored-By trailer).

### Task 1.2: Backend — routes + handlers in `api/mod.rs`

- [ ] **Step 1: Write the failing test.** Add a camelCase contract test to the `contract_tests` module at the bottom of `admin-api/src/api/mod.rs`:
```rust
    #[test]
    fn create_role_accepts_camelcase() {
        let b: CreateRole = serde_json::from_value(json!({
            "roleKey": "chat.viewer", "displayName": "Chat Viewer", "group": "chat"
        })).expect("camelCase CreateRole");
        assert_eq!(b.role_key, "chat.viewer");
        assert_eq!(b.display_name, "Chat Viewer");
        assert_eq!(b.group, "chat");
    }

    #[test]
    fn create_role_group_defaults_empty() {
        let b: CreateRole = serde_json::from_value(json!({
            "roleKey": "chat.viewer", "displayName": "Chat Viewer"
        })).expect("CreateRole without group");
        assert_eq!(b.group, "");
    }
```
- [ ] **Step 2: Run it, expect FAIL.** Run `cargo test -p admin-api --lib contract_tests` from `admin-api/`. Expect: `error[E0412]: cannot find type 'CreateRole' in this scope`.
- [ ] **Step 3: Implement.** In `admin-api/src/api/mod.rs`, add the routes to the router (after the existing `.route("/api/roles", get(list_roles))` line):
```rust
        .route("/api/roles", get(list_roles).post(create_role))
        .route("/api/roles/{roleKey}", delete(delete_role))
        .route("/api/roles/{roleKey}/holders", get(list_role_holders))
```
Replace the existing single `.route("/api/roles", get(list_roles))` line with the first line above (do not duplicate it). Then add the handlers + request struct after the existing `list_roles` handler:
```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRole { role_key: String, display_name: String, #[serde(default)] group: String }
async fn create_role(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateRole>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.create_role(&b.role_key, &b.display_name, &b.group).await?;
    Ok(Json(json!({ "ok": true })))
}

// DELETE cascades — strips this role from every grant (design §7).
async fn delete_role(_op: Operator, State(st): State<AppState>, Path(role_key): Path<String>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_role(&role_key).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn list_role_holders(_op: Operator, State(st): State<AppState>, Path(role_key): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_role_holders(&role_key).await? })))
}
```
- [ ] **Step 4: Run it, expect PASS.** Run `cargo test -p admin-api --lib` from `admin-api/`. Expect all contract tests pass, including the two new `create_role_*` tests, and the crate compiles (the new routes wired).
- [ ] **Step 5: Commit.** `git add admin-api/src/api/mod.rs && git commit -m "feat(admin-api): POST /api/roles, DELETE /api/roles/{roleKey}, GET holders routes"` (append the Co-Authored-By trailer).

### Task 1.3: Frontend — role + holder types

- [ ] **Step 1: Write the failing test.** This is a type-only change verified by `tsc`. First add a temporary assertion file `admin-web/lib/types.roles-check.ts`:
```ts
import type { RoleList, RoleHolder, RoleHolderList, GrantList } from "./types";
const _r: RoleList = { result: [{ key: "chat.user", displayName: "User" }] };
const _h: RoleHolder = { id: "g1", userId: "u1", roleKeys: ["chat.user"], displayName: "Alice" };
const _hl: RoleHolderList = { result: [_h] };
const _gl: GrantList = { result: [{ grantId: "g1", projectId: "p1", roleKeys: ["chat.user"] }] };
void _r; void _hl; void _gl;
```
- [ ] **Step 2: Run it, expect FAIL.** Run `npx tsc --noEmit` from `admin-web/`. Expect: `error TS2305: Module '"./types"' has no exported member 'RoleList'`.
- [ ] **Step 3: Implement.** Append to `admin-web/lib/types.ts` (the `Role` and `UserGrant` interfaces already exist — do not redefine them):
```ts
export interface RoleList {
  result: Role[];
}

export interface RoleHolder {
  id: string;
  userId: string;
  roleKeys: string[];
  displayName?: string;
  userName?: string;
}

export interface RoleHolderList {
  result: RoleHolder[];
}

export interface GrantList {
  result: UserGrant[];
}
```
- [ ] **Step 4: Run it, expect PASS.** Run `npx tsc --noEmit` from `admin-web/`. Expect no errors. Then delete the temporary file: `git rm -f --ignore-unmatch admin-web/lib/types.roles-check.ts` (or `del`/`rm` it).
- [ ] **Step 5: Commit.** `git add admin-web/lib/types.ts && git commit -m "feat(admin-web): role/holder/grant list types"` (append the Co-Authored-By trailer).

### Task 1.4: Frontend — CreateRoleDialog

- [ ] **Step 1: Write the failing test.** Add to `admin-web/e2e/smoke.spec.ts` inside the `authenticated operator flow` describe block (full impl in Task 1.10; here add the import-time reference that fails compile). Create the dialog test stub first:
```ts
  test("create then delete a role (cascade confirm)", async ({ page }) => {
    await page.goto("/roles");
    await expect(page.getByRole("heading", { name: "Roles" })).toBeVisible();
    const key = `pw.role.${Date.now()}`;
    await page.getByTestId("create-role").click();
    await page.getByLabel("Role key").fill(key);
    await page.getByLabel("Display name").fill("PW Role");
    await page.getByRole("button", { name: "Create" }).click();
    await page.getByPlaceholder(/filter by key/i).fill(key);
    await expect(page.getByText(key)).toBeVisible();
  });
```
- [ ] **Step 2: Run it, expect FAIL.** Run `npx playwright test smoke --list` from `admin-web/`. Without `ADMIN_IT=1` the body is skipped, but `--list` must enumerate it — confirm the new title appears: `create then delete a role (cascade confirm)`. (The route `/roles` does not exist yet; the test is skipped until 1.10 wiring + `ADMIN_IT=1`.)
- [ ] **Step 3: Implement.** Create `admin-web/components/roles/create-role-dialog.tsx` (mirrors `create-user-dialog.tsx` exactly):
```tsx
"use client";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger, DialogFooter,
} from "@/components/ui/dialog";
import {
  Form, FormControl, FormField, FormItem, FormLabel, FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { api, ApiError } from "@/lib/api";

const schema = z.object({
  roleKey: z.string().min(1),
  displayName: z.string().min(1),
  group: z.string().optional(),
});
type FormValues = z.infer<typeof schema>;

export function CreateRoleDialog({ onCreated }: { onCreated: () => void }) {
  const [open, setOpen] = useState(false);
  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { roleKey: "", displayName: "", group: "" },
  });

  async function onSubmit(values: FormValues) {
    try {
      await api.post("/api/roles", {
        roleKey: values.roleKey,
        displayName: values.displayName,
        group: values.group ?? "",
      });
      toast.success("Role created");
      setOpen(false);
      form.reset();
      onCreated();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Create failed");
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button data-testid="create-role">Create role</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>Create role</DialogTitle></DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <FormField control={form.control} name="roleKey" render={({ field }) => (
              <FormItem><FormLabel>Role key</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="displayName" render={({ field }) => (
              <FormItem><FormLabel>Display name</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="group" render={({ field }) => (
              <FormItem><FormLabel>Group (optional)</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <DialogFooter><Button type="submit">Create</Button></DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
```
- [ ] **Step 4: Run it, expect PASS.** Run `npx tsc --noEmit` from `admin-web/`. Expect no errors (the component compiles; the route page that consumes it lands in 1.7).
- [ ] **Step 5: Commit.** `git add admin-web/components/roles/create-role-dialog.tsx && git commit -m "feat(admin-web): CreateRoleDialog"` (append the Co-Authored-By trailer).

### Task 1.5: Frontend — HoldersDialog

- [ ] **Step 1: Write the failing test.** Add a temporary compile-check `admin-web/components/roles/holders-dialog.check.ts`:
```ts
import { HoldersDialog } from "./holders-dialog";
void HoldersDialog;
```
- [ ] **Step 2: Run it, expect FAIL.** Run `npx tsc --noEmit` from `admin-web/`. Expect: `error TS2307: Cannot find module './holders-dialog'`.
- [ ] **Step 3: Implement.** Create `admin-web/components/roles/holders-dialog.tsx`:
```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { api, ApiError } from "@/lib/api";
import type { Role, RoleHolder, RoleHolderList } from "@/lib/types";

export function HoldersDialog({
  role, open, onOpenChange,
}: {
  role: Role | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const [holders, setHolders] = useState<RoleHolder[]>([]);

  const load = useCallback(async () => {
    if (!role) return;
    try {
      // roleKey is part of the path -> encode (design §7).
      const list = await api.get<RoleHolderList>(
        `/api/roles/${encodeURIComponent(role.key)}/holders`,
      );
      setHolders(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error(e instanceof ApiError ? e.message : "Failed to load holders");
      }
    }
  }, [role]);

  useEffect(() => {
    if (open) load();
    else setHolders([]);
  }, [open, load]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Holders of {role?.key}</DialogTitle>
        </DialogHeader>
        {holders.length === 0 ? (
          <p className="text-sm text-muted-foreground">No one holds this role.</p>
        ) : (
          <ul className="space-y-2">
            {holders.map((h) => (
              <li key={h.id} className="flex items-center justify-between gap-2">
                <span className="text-sm">{h.displayName ?? h.userName ?? h.userId}</span>
                <Badge variant="secondary">{h.userId}</Badge>
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}
```
- [ ] **Step 4: Run it, expect PASS.** Run `npx tsc --noEmit` from `admin-web/`. Expect no errors. Delete the temp file: `rm admin-web/components/roles/holders-dialog.check.ts` (PowerShell: `Remove-Item admin-web/components/roles/holders-dialog.check.ts`).
- [ ] **Step 5: Commit.** `git add admin-web/components/roles/holders-dialog.tsx && git commit -m "feat(admin-web): HoldersDialog (encodeURIComponent roleKey)"` (append the Co-Authored-By trailer).

### Task 1.6: Frontend — role columns

- [ ] **Step 1: Write the failing test.** Add a temporary compile-check `admin-web/components/roles/columns.check.ts`:
```ts
import { buildRoleColumns } from "./columns";
const cols = buildRoleColumns({ onHolders: () => {}, onDelete: () => {} });
void cols;
```
- [ ] **Step 2: Run it, expect FAIL.** Run `npx tsc --noEmit` from `admin-web/`. Expect: `error TS2307: Cannot find module './columns'`.
- [ ] **Step 3: Implement.** Create `admin-web/components/roles/columns.tsx` (mirrors `components/users/columns.tsx`):
```tsx
"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { MoreHorizontal } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { Role } from "@/lib/types";

export interface RoleColumnHandlers {
  onHolders: (r: Role) => void;
  onDelete: (r: Role) => void;
}

export function buildRoleColumns(h: RoleColumnHandlers): ColumnDef<Role>[] {
  return [
    { accessorKey: "key", header: "Key" },
    { accessorKey: "displayName", header: "Display name" },
    {
      accessorKey: "group", header: "Group",
      cell: ({ row }) =>
        row.original.group
          ? <Badge variant="secondary">{row.original.group}</Badge>
          : <span className="text-muted-foreground">—</span>,
    },
    {
      id: "actions",
      cell: ({ row }) => {
        const r = row.original;
        return (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" className="h-8 w-8 p-0">
                <span className="sr-only">Open menu</span>
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuLabel>Actions</DropdownMenuLabel>
              <DropdownMenuItem data-testid="role-holders"
                onSelect={() => h.onHolders(r)}>View holders</DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem data-testid="role-delete"
                className="text-destructive" onSelect={() => h.onDelete(r)}>
                Delete (cascades)
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        );
      },
    },
  ];
}
```
- [ ] **Step 4: Run it, expect PASS.** Run `npx tsc --noEmit` from `admin-web/`. Expect no errors. Delete the temp file: `rm admin-web/components/roles/columns.check.ts`.
- [ ] **Step 5: Commit.** `git add admin-web/components/roles/columns.tsx && git commit -m "feat(admin-web): role table columns + actions"` (append the Co-Authored-By trailer).

### Task 1.7: Frontend — Roles page + NAV entry

- [ ] **Step 1: Write the failing test.** Add a temporary compile-check `admin-web/app/(dash)/roles/page.check.ts`:
```ts
import RolesPage from "./page";
void RolesPage;
```
- [ ] **Step 2: Run it, expect FAIL.** Run `npx tsc --noEmit` from `admin-web/`. Expect: `error TS2307: Cannot find module './page'`.
- [ ] **Step 3: Implement.** Create `admin-web/app/(dash)/roles/page.tsx` (mirrors `users/page.tsx`, minus the shell chrome which Phase 0 owns; the cascade ConfirmDialog reuses the shared `components/users/confirm-dialog.tsx`):
```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { buildRoleColumns } from "@/components/roles/columns";
import { CreateRoleDialog } from "@/components/roles/create-role-dialog";
import { HoldersDialog } from "@/components/roles/holders-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { api, ApiError } from "@/lib/api";
import type { Role, RoleList } from "@/lib/types";

export default function RolesPage() {
  const [roles, setRoles] = useState<Role[]>([]);
  const [holdersTarget, setHoldersTarget] = useState<Role | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Role | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.get<RoleList>("/api/roles");
      setRoles(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load roles");
      }
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      // roleKey is a path param -> encode (design §7).
      await api.del(`/api/roles/${encodeURIComponent(deleteTarget.key)}`);
      toast.success("Role deleted");
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    } finally {
      setDeleteTarget(null);
      load();
    }
  }

  const columns = buildRoleColumns({
    onHolders: setHoldersTarget,
    onDelete: setDeleteTarget,
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Roles</h1>
        <CreateRoleDialog onCreated={load} />
      </div>
      <DataTable columns={columns} data={roles}
        filterColumn="key" filterPlaceholder="Filter by key..."
        emptyMessage="No roles." />
      <HoldersDialog role={holdersTarget} open={!!holdersTarget}
        onOpenChange={(o) => !o && setHoldersTarget(null)} />
      <ConfirmDialog open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete role?"
        description="This cascades — the role is stripped from every user grant that holds it. Deleting chat.admin can lock operators out of admin-web. This cannot be undone."
        confirmLabel="Delete role" onConfirm={confirmDelete} />
    </div>
  );
}
```
Then append the Roles entry to the `NAV` array (Phase 0's single nav source — `components/shell/Sidebar.tsx` or `components/shell/nav.ts`). Add `ShieldCheck` to the existing lucide import and append:
```tsx
  { icon: ShieldCheck, label: "Roles", href: "/roles", match: "/roles" },
```
- [ ] **Step 4: Run it, expect PASS.** Run `npx tsc --noEmit` from `admin-web/`. Expect no errors. Delete the temp file: `rm "admin-web/app/(dash)/roles/page.check.ts"`. (The `emptyMessage` prop on DataTable is a Phase 0 prerequisite — if `tsc` errors that the prop is unknown, Phase 0 is incomplete; do not remove the prop.)
- [ ] **Step 5: Commit.** `git add "admin-web/app/(dash)/roles/page.tsx" admin-web/components/shell && git commit -m "feat(admin-web): Roles page + NAV entry"` (append the Co-Authored-By trailer).

### Task 1.8: Frontend — GrantsDialog (the one-grant-per-project branch)

- [ ] **Step 1: Write the failing test.** Add a temporary compile-check `admin-web/components/users/grants-dialog.check.ts`:
```ts
import { GrantsDialog } from "./grants-dialog";
void GrantsDialog;
```
- [ ] **Step 2: Run it, expect FAIL.** Run `npx tsc --noEmit` from `admin-web/`. Expect: `error TS2307: Cannot find module './grants-dialog'`.
- [ ] **Step 3: Implement.** Create `admin-web/components/users/grants-dialog.tsx`. This is the **critical** one-grant-per-project branch: load the user's existing grant for the project (if any) + the full role catalog; on save, compute the next `roleKeys` set and branch **POST** (no grant yet + roles selected), **PUT** (grant exists + roles selected), **DELETE** (grant exists + nothing selected). `assumes the single-project model (one grant per user+project)`:
```tsx
"use client";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import type { GrantList, Role, RoleList, User, UserGrant } from "@/lib/types";

export function GrantsDialog({
  user, open, onOpenChange, onSaved,
}: {
  user: User | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
}) {
  const [roles, setRoles] = useState<Role[]>([]);
  const [grant, setGrant] = useState<UserGrant | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!user) return;
    try {
      const [roleList, grantList] = await Promise.all([
        api.get<RoleList>("/api/roles"),
        api.get<GrantList>(`/api/users/${user.id}/grants`),
      ]);
      setRoles(roleList.result);
      // One grant per (user, project): the first (only) grant, if any.
      const g = grantList.result[0] ?? null;
      setGrant(g);
      setSelected(new Set(g?.roleKeys ?? []));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error(e instanceof ApiError ? e.message : "Failed to load grants");
      }
    }
  }, [user]);

  useEffect(() => {
    if (open) load();
    else { setRoles([]); setGrant(null); setSelected(new Set()); }
  }, [open, load]);

  function toggle(key: string, on: boolean) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (on) next.add(key); else next.delete(key);
      return next;
    });
  }

  const nextKeys = useMemo(() => Array.from(selected), [selected]);

  async function onSave() {
    if (!user) return;
    setSaving(true);
    try {
      // The one-grant-per-project branch (design §7):
      if (!grant && nextKeys.length > 0) {
        // No grant yet, roles chosen -> POST create.
        await api.post(`/api/users/${user.id}/grants`, { roleKeys: nextKeys });
      } else if (grant && nextKeys.length > 0) {
        // Grant exists, roles chosen -> PUT replace the whole roleKeys set.
        await api.put(`/api/users/${user.id}/grants/${grant.grantId}`, { roleKeys: nextKeys });
      } else if (grant && nextKeys.length === 0) {
        // Grant exists, nothing chosen -> DELETE revoke the whole grant.
        await api.del(`/api/users/${user.id}/grants/${grant.grantId}`);
      }
      // (no grant && nothing chosen): nothing to do.
      toast.success("Access updated");
      onOpenChange(false);
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Access for {user?.userName}</DialogTitle>
        </DialogHeader>
        <div className="space-y-2">
          {roles.length === 0 ? (
            <p className="text-sm text-muted-foreground">No roles defined.</p>
          ) : roles.map((r) => (
            <label key={r.key} className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={selected.has(r.key)}
                onCheckedChange={(v) => toggle(r.key, v === true)}
                data-testid={`grant-role-${r.key}`}
              />
              <span>{r.displayName} <span className="text-muted-foreground">({r.key})</span></span>
            </label>
          ))}
        </div>
        <DialogFooter>
          <Button onClick={onSave} disabled={saving} data-testid="grants-save">
            Save access
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```
**Step 0 (only if `components/ui/checkbox.tsx` is missing — it is a Phase 0 prerequisite):** run `npx shadcn@latest add checkbox` from `admin-web/` before Step 4 so `@/components/ui/checkbox` resolves.
- [ ] **Step 4: Run it, expect PASS.** Run `npx tsc --noEmit` from `admin-web/`. Expect no errors. Delete the temp file: `rm admin-web/components/users/grants-dialog.check.ts`.
- [ ] **Step 5: Commit.** `git add admin-web/components/users/grants-dialog.tsx admin-web/components/ui/checkbox.tsx && git commit -m "feat(admin-web): GrantsDialog — one-grant-per-project POST/PUT/DELETE branch"` (append the Co-Authored-By trailer; drop the checkbox.tsx path if Phase 0 already committed it).

### Task 1.9: Frontend — wire Grants into Users page + columns

- [ ] **Step 1: Write the failing test.** Add a temporary compile-check `admin-web/components/users/columns.grants-check.ts`:
```ts
import { buildColumns } from "./columns";
const cols = buildColumns({
  onEdit: () => {}, onDelete: () => {}, onLifecycle: () => {}, onGrants: () => {},
});
void cols;
```
- [ ] **Step 2: Run it, expect FAIL.** Run `npx tsc --noEmit` from `admin-web/`. Expect: `error TS2353: Object literal may only specify known properties, and 'onGrants' does not exist in type 'ColumnHandlers'`.
- [ ] **Step 3: Implement.** In `admin-web/components/users/columns.tsx`, extend the handlers interface and add the menu item. Change the `ColumnHandlers` interface:
```tsx
export interface ColumnHandlers {
  onEdit: (u: User) => void;
  onDelete: (u: User) => void;
  onLifecycle: (u: User, action: Lifecycle) => void;
  onGrants: (u: User) => void;
}
```
Then add an "Access (grants)" item to the dropdown, immediately after the `Edit profile` block (before the first lifecycle item):
```tsx
              <DropdownMenuItem data-testid="action-grants"
                onSelect={() => h.onGrants(u)}>Access (grants)</DropdownMenuItem>
```
Then in `admin-web/app/(dash)/users/page.tsx`, import the dialog and add state + handler. Add the import near the other component imports:
```tsx
import { GrantsDialog } from "@/components/users/grants-dialog";
```
Add the state next to `deleteTarget`:
```tsx
  const [grantsTarget, setGrantsTarget] = useState<User | null>(null);
```
Add `onGrants: setGrantsTarget,` to the `buildColumns({...})` call. Render the dialog next to the others (before the closing `</main>` or the page's outer container close):
```tsx
      <GrantsDialog user={grantsTarget} open={!!grantsTarget}
        onOpenChange={(o) => !o && setGrantsTarget(null)} onSaved={load} />
```
- [ ] **Step 4: Run it, expect PASS.** Run `npx tsc --noEmit` from `admin-web/`. Expect no errors. Delete the temp file: `rm admin-web/components/users/columns.grants-check.ts`.
- [ ] **Step 5: Commit.** `git add "admin-web/app/(dash)/users/page.tsx" admin-web/components/users/columns.tsx && git commit -m "feat(admin-web): wire per-user Access (grants) dialog into Users page"` (append the Co-Authored-By trailer).

### Task 1.10: E2E — Roles create/delete + grant round-trip

- [ ] **Step 1: Write the failing test.** The role create/delete test was stubbed in 1.4; now complete it (add the delete + cascade-confirm) and add the grant round-trip. Replace the stub `create then delete a role` test in `admin-web/e2e/smoke.spec.ts` with the full version, and add a second test, both inside the `authenticated operator flow` describe block:
```ts
  test("create then delete a role (cascade confirm)", async ({ page }) => {
    await page.goto("/roles");
    await expect(page.getByRole("heading", { name: "Roles" })).toBeVisible();
    const key = `pw.role.${Date.now()}`;
    await page.getByTestId("create-role").click();
    await page.getByLabel("Role key").fill(key);
    await page.getByLabel("Display name").fill("PW Role");
    await page.getByRole("button", { name: "Create" }).click();

    await page.getByPlaceholder(/filter by key/i).fill(key);
    await expect(page.getByText(key)).toBeVisible();

    // Delete via the row action -> cascade confirm.
    await page.getByRole("row", { name: new RegExp(key) })
      .getByRole("button", { name: /open menu/i }).click();
    await page.getByTestId("role-delete").click();
    await page.getByRole("button", { name: "Delete role" }).click();
    await expect(page.getByText(key)).toHaveCount(0);
  });

  test("grant assign then revoke round-trip", async ({ page }) => {
    // Create a throwaway machine user, then toggle a grant on/off via the
    // one-grant-per-project branch (POST create -> DELETE revoke-all).
    await page.goto("/users");
    const uname = `pw-grant-${Date.now()}`;
    await page.getByTestId("create-user").click();
    await page.getByRole("combobox").click();
    await page.getByRole("option", { name: "Machine" }).click();
    await page.getByLabel("Username").fill(uname);
    await page.getByLabel("Display name").fill(uname);
    await page.getByRole("button", { name: "Create" }).click();
    await page.getByPlaceholder(/filter by username/i).fill(uname);
    await expect(page.getByText(uname)).toBeVisible();

    // Open Access (grants), assign chat.user (POST create grant).
    await page.getByRole("row", { name: new RegExp(uname) })
      .getByRole("button", { name: /open menu/i }).click();
    await page.getByTestId("action-grants").click();
    await page.getByTestId("grant-role-chat.user").click();
    await page.getByTestId("grants-save").click();
    await expect(page.getByText("Access updated")).toBeVisible();

    // Re-open, unselect everything, save (DELETE revoke-all). The dialog
    // reloads the now-checked chat.user; unchecking + save deletes the grant.
    await page.getByRole("row", { name: new RegExp(uname) })
      .getByRole("button", { name: /open menu/i }).click();
    await page.getByTestId("action-grants").click();
    await page.getByTestId("grant-role-chat.user").click(); // uncheck
    await page.getByTestId("grants-save").click();
    await expect(page.getByText("Access updated")).toBeVisible();
  });
```
- [ ] **Step 2: Run it, expect FAIL (or skip without the stack).** Without the stack: run `npx playwright test smoke --list` from `admin-web/` and confirm both titles enumerate: `create then delete a role (cascade confirm)` and `grant assign then revoke round-trip`. With the stack up: `ADMIN_IT=1 ADMIN_IT_USER=... ADMIN_IT_PASS=... npx playwright test smoke` — expect the two new tests FAIL only if a wiring step (Tasks 1.7/1.9) is missing; otherwise they pass. (PowerShell: prefix with `$env:ADMIN_IT='1'; $env:ADMIN_IT_USER=...; $env:ADMIN_IT_PASS=...;`.)
- [ ] **Step 3: Implement.** No new product code — this task is the e2e spec itself (Step 1). If a test reveals a real defect, fix the underlying component (never weaken the assertion), per the project's no-dirty-fix rule.
- [ ] **Step 4: Run it, expect PASS.** With the stack up: `ADMIN_IT=1 ADMIN_IT_USER=... ADMIN_IT_PASS=... npx playwright test smoke` from `admin-web/`. Expect all tests, including the two new ones, pass. Without the stack, `npx playwright test smoke` passes with the two new tests reported as skipped.
- [ ] **Step 5: Commit.** `git add admin-web/e2e/smoke.spec.ts && git commit -m "test(admin-web): e2e role create/delete + grant assign/revoke round-trip"` (append the Co-Authored-By trailer).

### Task 1.11: Backend — full crate gate + camelCase contract verification

- [ ] **Step 1: Write the failing test.** Confirm the whole backend round-trips (no regressions from the new routes/handlers). No new test file — this gate runs the entire suite. First, run it to capture the current green baseline.
- [ ] **Step 2: Run it, expect PASS-then-prove.** Run `cargo test -p admin-api` from `admin-api/`. Expect all tests pass: the `grants` module tests (`roles_without_*` + `holders_query_filters_by_project_and_role_anded`) and the `contract_tests` (`create_role_accepts_camelcase`, `create_role_group_defaults_empty`, plus the pre-existing user/grant ones).
- [ ] **Step 3: Implement.** If anything fails, root-cause it (e.g. a missing `use`, a router double-registration of `/api/roles`). Do not silence — fix the cause. Confirm `/api/roles` appears exactly once in the router (the `get(list_roles).post(create_role)` line replaced the old `get(list_roles)` line; there must be no leftover duplicate).
- [ ] **Step 4: Run it, expect PASS.** Run `cargo clippy -p admin-api --all-targets -- -D warnings` then `cargo test -p admin-api` from `admin-api/`. Expect zero warnings and all tests green.
- [ ] **Step 5: Commit.** Only if Step 3 changed code: `git add admin-api/src && git commit -m "chore(admin-api): clippy-clean roles routes, single /api/roles registration"` (append the Co-Authored-By trailer). If nothing changed, skip the commit.

## Phase 2: OIDC Applications

Builds the OIDC-client CRUD surface for the "Chat" app (the project's apps tab): list apps, view/edit an app's `oidc_config` (redirectUris, grantTypes, responseTypes, appType, authMethod) via read-modify-write, create with a one-time secret reveal, rotate the client secret (one-time reveal + breakage confirm), and delete (with a "changing redirectUris can break a live login" confirm). The secret one-time-reveal invariant (spec §3): the BFF passes the Zitadel `Value` straight through, untouched, never logged — exactly like `create_json_key`/`generate_secret` in `keys.rs`.

**Task 2.1 runs FIRST and is a hard gate:** it confirms the two spec §8 `verified:false` endpoints (`PUT .../apps/{appId}/oidc_config` and `POST .../apps/{appId}/oidc_config/_generate_client_secret`) against a **live Zitadel** (`ADMIN_IT=1`) before any handler relies on them. Do not write `apps.rs` methods 4 and 5 until 2.1 passes and prints the verified paths/bodies.

Depends on Phase 0 (the `(dash)/layout.tsx` shell + `NAV` array + `card`/`switch`/`checkbox` primitives + the `emptyMessage` DataTable prop). This phase reuses the shell and the parameterized empty message but does **not** require `card`/`switch`/`checkbox` (it uses `dialog`, `input`, `form`, `select`, `data-table`, `dropdown-menu`, `alert-dialog`, all already present).

**Files:**

- `admin-api/tests/integration.rs` (Modify) — add the `ADMIN_IT`-gated live test that verifies the two unknown OIDC endpoints (Task 2.1) and the full apps lifecycle (Task 2.7).
- `admin-api/src/zitadel/apps.rs` (Create) — 6 `ZitadelClient` methods: `list_apps`, `create_oidc_app`, `get_app`, `update_oidc_config`, `regenerate_app_secret`, `delete_app`.
- `admin-api/src/zitadel/mod.rs` (Modify) — add `pub mod apps;`.
- `admin-api/src/api/mod.rs` (Modify) — add the 6 `/api/apps*` routes + handlers (`CreateOidcApp`/`UpdateOidcConfig` camelCase contract structs + tests).
- `admin-web/lib/types.ts` (Modify) — add `OidcApp`, `OidcAppList`, `OidcConfig`, `CreateOidcAppInput`, `AppSecret`, and the OIDC enum string-union types.
- `admin-web/lib/oidc.ts` (Create) — shared OIDC enum option arrays + a pure `appToConfigForm`/`formToConfigBody` mapper (DRY; unit-tested) used by create + edit.
- `admin-web/components/apps/columns.tsx` (Create) — DataTable columns for the apps list + per-row lifecycle `DropdownMenu` (edit / rotate secret / delete).
- `admin-web/components/apps/secret-reveal-dialog.tsx` (Create) — the one-time secret reveal `Dialog` (copy-now affordance, "won't be shown again").
- `admin-web/components/apps/app-form-dialog.tsx` (Create) — create/edit `Dialog` (react-hook-form + zod) for the full `oidc_config`.
- `admin-web/app/(dash)/applications/page.tsx` (Create) — the thin list page mirroring `users/page.tsx`.
- `admin-web/e2e/smoke.spec.ts` (Modify) — extend with the App-create **secret-reveal-once** Playwright assertion.

---

### Task 2.1: Verify the two `verified:false` OIDC endpoints against a live Zitadel (GATE)

This task discharges spec §14 risk #2 before any code depends on it. It calls the two unknown endpoints with the SA token through the existing `post_json`/`put_json` helpers and asserts the real response shape. Run it with `ADMIN_IT=1` + the Zitadel env (the same setup `it_mint_management_token` uses).

- [ ] **Step 1: Write the failing test** — append to `admin-api/tests/integration.rs`:

```rust
/// GATE for design §8: the two endpoints marked verified:false —
/// PUT .../apps/{appId}/oidc_config and
/// POST .../apps/{appId}/oidc_config/_generate_client_secret. We create a
/// throwaway OIDC app (the provisioner-proven create path), then exercise the
/// two unknowns and assert the live response shapes before any handler relies
/// on them. Driven straight through ZitadelClient (the source of truth).
#[tokio::test]
async fn it_verify_oidc_config_put_and_secret_regen() {
    if !it_enabled() {
        eprintln!("ADMIN_IT!=1 — skipping OIDC endpoint verification (design §8)");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let z = admin_client(cfg, http);

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let name = format!("it-oidc-app-{suffix}");

    // create (provisioner-proven, design §8 ✅): WEB+BASIC yields a clientSecret.
    let created = z.create_oidc_app(
        &name,
        &["https://example.localhost/callback".into()],
        &["OIDC_RESPONSE_TYPE_CODE".into()],
        &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
        "OIDC_APP_TYPE_WEB",
        "OIDC_AUTH_METHOD_TYPE_BASIC",
    ).await.expect("create_oidc_app");
    let app_id = created.get("appId").and_then(|v| v.as_str())
        .expect("create returns appId").to_string();
    assert!(created.get("clientId").and_then(|v| v.as_str()).is_some(),
        "create returns clientId");
    assert!(created.get("clientSecret").and_then(|v| v.as_str()).is_some(),
        "WEB+BASIC create returns clientSecret once");

    // get (design §8 ✅): reads back the oidc_config under the app.
    let app = z.get_app(&app_id).await.expect("get_app");
    assert!(app.get("oidcConfig").is_some() || app.get("app").is_some(),
        "get_app returns the app with its oidcConfig");

    // UNKNOWN #1 — PUT .../apps/{appId}/oidc_config (read-modify-write whole config).
    z.update_oidc_config(
        &app_id,
        &["https://example.localhost/callback".into(),
          "https://example.localhost/cb2".into()],
        &["OIDC_RESPONSE_TYPE_CODE".into()],
        &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
        "OIDC_APP_TYPE_WEB",
        "OIDC_AUTH_METHOD_TYPE_BASIC",
    ).await.expect("update_oidc_config (PUT oidc_config) — design §8 unknown #1");

    // UNKNOWN #2 — POST .../apps/{appId}/oidc_config/_generate_client_secret.
    let regen = z.regenerate_app_secret(&app_id).await
        .expect("regenerate_app_secret — design §8 unknown #2");
    assert!(regen.get("clientSecret").and_then(|v| v.as_str()).is_some(),
        "_generate_client_secret returns clientSecret once");

    // cleanup (design §8 ✅).
    z.delete_app(&app_id).await.expect("delete_app");
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p llm-chat-admin-api --test integration it_verify_oidc_config_put_and_secret_regen` → fails to compile: `error[E0599]: no method named \`create_oidc_app\` found for struct \`ZitadelClient\`` (the `apps.rs` methods don't exist yet).
- [ ] **Step 3: Implement** — none yet; the methods land in Task 2.2 (this is the gate test that pulls them into existence). Proceed to 2.2, then return here.
- [ ] **Step 4: Run it, expect PASS** — after 2.2: `set ADMIN_IT=1` (plus the Zitadel env from `config.md`) then `cargo test -p llm-chat-admin-api --test integration it_verify_oidc_config_put_and_secret_regen -- --nocapture`. Expect the test to pass; if `update_oidc_config` 4xx's, the real path/body differs from the spec table — fix `apps.rs` method 4/5 to the path the live instance accepts and re-run. **Do not proceed past this gate until it is green.**
- [ ] **Step 5: Commit** —
```
git add admin-api/tests/integration.rs
git commit -m "test(admin-api): ADMIN_IT gate verifying the two unknown OIDC endpoints (§8)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2.2: `zitadel/apps.rs` — the 6 OIDC-app client methods

Mirrors `users.rs`/`keys.rs` exactly: thin `pub async fn`s over `post_json`/`put_json`/`get_json`/`delete`, `project_id` from `self.cfg.project_id`, camelCase preserved. Create/regenerate stream the full `Value` straight through (secret lives there once — never logged), like `create_json_key`. The create body is the provisioner-proven shape from `provision.py::create_admin_oidc_app`. The two unknown paths (methods 4 & 5) are written to the spec §8 table and **confirmed live by Task 2.1**.

- [ ] **Step 1: Write the failing test** — the unit assertions live in `apps.rs` itself (pure path/body construction needs no live call; the live shapes are Task 2.1). Add at the bottom of the new file (Step 3 creates the file with this test included):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oidc_create_body_carries_the_provisioner_proven_fields() {
        let body = oidc_create_body(
            "Chat",
            &["https://x/cb".into()],
            &["OIDC_RESPONSE_TYPE_CODE".into()],
            &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
            "OIDC_APP_TYPE_WEB",
            "OIDC_AUTH_METHOD_TYPE_BASIC",
        );
        assert_eq!(body["name"], "Chat");
        assert_eq!(body["redirectUris"][0], "https://x/cb");
        assert_eq!(body["responseTypes"][0], "OIDC_RESPONSE_TYPE_CODE");
        assert_eq!(body["grantTypes"][0], "OIDC_GRANT_TYPE_AUTHORIZATION_CODE");
        assert_eq!(body["appType"], "OIDC_APP_TYPE_WEB");
        assert_eq!(body["authMethodType"], "OIDC_AUTH_METHOD_TYPE_BASIC");
        assert_eq!(body["accessTokenType"], "OIDC_TOKEN_TYPE_JWT");
    }

    #[test]
    fn oidc_update_body_omits_name_but_keeps_the_full_config() {
        let body = oidc_update_body(
            &["https://x/cb".into()],
            &["OIDC_RESPONSE_TYPE_CODE".into()],
            &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
            "OIDC_APP_TYPE_NATIVE",
            "OIDC_AUTH_METHOD_TYPE_NONE",
        );
        assert!(body.get("name").is_none(), "PUT oidc_config takes no name");
        assert_eq!(body["appType"], "OIDC_APP_TYPE_NATIVE");
        assert_eq!(body["authMethodType"], "OIDC_AUTH_METHOD_TYPE_NONE");
        assert_eq!(body["redirectUris"][0], "https://x/cb");
    }
}
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p llm-chat-admin-api oidc_create_body_carries` → `error[E0583]: file not found for module \`apps\`` / `cannot find function \`oidc_create_body\`` (file/module not created yet).
- [ ] **Step 3: Implement** — create `admin-api/src/zitadel/apps.rs`:

```rust
//! OIDC application (login client) CRUD within a project (design §8).
//! An "App" = a Zitadel Project; these are the OIDC *clients* under it.
//! v1 Management API. clientSecret is returned ONCE by create + regenerate and
//! is streamed straight through untouched — NEVER logged (design §3 invariant,
//! same contract as keys::create_json_key). The two endpoints marked
//! verified:false in §8 (oidc_config PUT, _generate_client_secret) are confirmed
//! live by tests/integration.rs::it_verify_oidc_config_put_and_secret_regen.

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

/// PURE: the create body — the provisioner-proven shape (provision.py
/// create_admin_oidc_app). accessTokenType is the OIDC app enum
/// OIDC_TOKEN_TYPE_JWT (NOT the machine ACCESS_TOKEN_TYPE_JWT — §enum trap).
fn oidc_create_body(
    name: &str, redirect_uris: &[String], response_types: &[String],
    grant_types: &[String], app_type: &str, auth_method: &str,
) -> Value {
    json!({
        "name": name,
        "redirectUris": redirect_uris,
        "responseTypes": response_types,
        "grantTypes": grant_types,
        "appType": app_type,
        "authMethodType": auth_method,
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
        "devMode": true,
        "accessTokenRoleAssertion": true,
        "idTokenRoleAssertion": true,
    })
}

/// PURE: the PUT oidc_config body — read-modify-write the full config (design
/// §8). No `name` (that's an app-level field, not part of oidc_config).
fn oidc_update_body(
    redirect_uris: &[String], response_types: &[String],
    grant_types: &[String], app_type: &str, auth_method: &str,
) -> Value {
    json!({
        "redirectUris": redirect_uris,
        "responseTypes": response_types,
        "grantTypes": grant_types,
        "appType": app_type,
        "authMethodType": auth_method,
        "accessTokenType": "OIDC_TOKEN_TYPE_JWT",
        "devMode": true,
        "accessTokenRoleAssertion": true,
        "idTokenRoleAssertion": true,
    })
}

impl ZitadelClient {
    /// List apps: POST /management/v1/projects/{pid}/apps/_search (§8 ✅).
    pub async fn list_apps(&self) -> Result<Vec<Value>, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/_search", self.cfg.issuer, pid);
        let v = self.post_json(&url, &json!({})).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Create an OIDC app: POST /management/v1/projects/{pid}/apps/oidc (§8 ✅,
    /// provisioner-proven). Returns the FULL response — clientId + clientSecret
    /// (shown ONCE) live here; streamed straight through, never logged.
    pub async fn create_oidc_app(
        &self, name: &str, redirect_uris: &[String], response_types: &[String],
        grant_types: &[String], app_type: &str, auth_method: &str,
    ) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/oidc", self.cfg.issuer, pid);
        let body = oidc_create_body(name, redirect_uris, response_types, grant_types, app_type, auth_method);
        self.post_json(&url, &body).await
    }

    /// Get one app: GET /management/v1/projects/{pid}/apps/{appId} (§8 ✅).
    pub async fn get_app(&self, app_id: &str) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/{}", self.cfg.issuer, pid, app_id);
        self.get_json(&url).await
    }

    /// Replace the whole oidc_config: PUT
    /// /management/v1/projects/{pid}/apps/{appId}/oidc_config (§8 unknown #1,
    /// confirmed live by it_verify_oidc_config_put_and_secret_regen).
    pub async fn update_oidc_config(
        &self, app_id: &str, redirect_uris: &[String], response_types: &[String],
        grant_types: &[String], app_type: &str, auth_method: &str,
    ) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/{}/oidc_config", self.cfg.issuer, pid, app_id);
        let body = oidc_update_body(redirect_uris, response_types, grant_types, app_type, auth_method);
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Regenerate the client secret: POST
    /// /management/v1/projects/{pid}/apps/{appId}/oidc_config/_generate_client_secret
    /// (§8 unknown #2, confirmed live). Returns clientSecret ONCE — straight
    /// through, never logged.
    pub async fn regenerate_app_secret(&self, app_id: &str) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!(
            "{}/management/v1/projects/{}/apps/{}/oidc_config/_generate_client_secret",
            self.cfg.issuer, pid, app_id);
        self.post_json(&url, &json!({})).await
    }

    /// Delete an app: DELETE /management/v1/projects/{pid}/apps/{appId} (§8 ✅).
    pub async fn delete_app(&self, app_id: &str) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}/apps/{}", self.cfg.issuer, pid, app_id);
        self.delete(&url).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oidc_create_body_carries_the_provisioner_proven_fields() {
        let body = oidc_create_body(
            "Chat",
            &["https://x/cb".into()],
            &["OIDC_RESPONSE_TYPE_CODE".into()],
            &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
            "OIDC_APP_TYPE_WEB",
            "OIDC_AUTH_METHOD_TYPE_BASIC",
        );
        assert_eq!(body["name"], "Chat");
        assert_eq!(body["redirectUris"][0], "https://x/cb");
        assert_eq!(body["responseTypes"][0], "OIDC_RESPONSE_TYPE_CODE");
        assert_eq!(body["grantTypes"][0], "OIDC_GRANT_TYPE_AUTHORIZATION_CODE");
        assert_eq!(body["appType"], "OIDC_APP_TYPE_WEB");
        assert_eq!(body["authMethodType"], "OIDC_AUTH_METHOD_TYPE_BASIC");
        assert_eq!(body["accessTokenType"], "OIDC_TOKEN_TYPE_JWT");
    }

    #[test]
    fn oidc_update_body_omits_name_but_keeps_the_full_config() {
        let body = oidc_update_body(
            &["https://x/cb".into()],
            &["OIDC_RESPONSE_TYPE_CODE".into()],
            &["OIDC_GRANT_TYPE_AUTHORIZATION_CODE".into()],
            "OIDC_APP_TYPE_NATIVE",
            "OIDC_AUTH_METHOD_TYPE_NONE",
        );
        assert!(body.get("name").is_none(), "PUT oidc_config takes no name");
        assert_eq!(body["appType"], "OIDC_APP_TYPE_NATIVE");
        assert_eq!(body["authMethodType"], "OIDC_AUTH_METHOD_TYPE_NONE");
        assert_eq!(body["redirectUris"][0], "https://x/cb");
    }
}
```

Then add the module declaration to `admin-api/src/zitadel/mod.rs` — after the `pub mod users;` line, insert:

```rust
pub mod apps;
```

(Final ordering: `pub mod apps;` sits alphabetically before `error`; place it as the first `pub mod` line so the list reads `apps, error, grants, keys, model, token, users`.)

- [ ] **Step 4: Run it, expect PASS** — `cargo test -p llm-chat-admin-api oidc_create_body oidc_update_body` → both pass. Then return to **Task 2.1 Step 4** and run the `ADMIN_IT=1` gate to confirm the live endpoints.
- [ ] **Step 5: Commit** —
```
git add admin-api/src/zitadel/apps.rs admin-api/src/zitadel/mod.rs
git commit -m "feat(admin-api): zitadel/apps.rs — OIDC app CRUD (§8), secret pass-through

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2.3: `/api/apps*` routes + handlers in `api/mod.rs`

Six thin handlers behind the `Operator` extractor, returning the Zitadel JSON passed through (camelCase preserved), exactly like the existing user/grant/key handlers. Create + regenerate return the full Zitadel `Value` straight to the operator (secret once), mirroring `create_key`/`generate_secret`. Camelcase contract structs (`CreateOidcApp`, `UpdateOidcConfig`) get the same `#[serde(rename_all = "camelCase")]` + a unit test as `CreateHuman`/`AddGrant`.

- [ ] **Step 1: Write the failing test** — add to the `contract_tests` module at the bottom of `admin-api/src/api/mod.rs`:

```rust
    #[test]
    fn create_oidc_app_accepts_camelcase() {
        let b: CreateOidcApp = serde_json::from_value(json!({
            "name": "Chat",
            "redirectUris": ["https://x/cb"],
            "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
            "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
            "appType": "OIDC_APP_TYPE_WEB",
            "authMethodType": "OIDC_AUTH_METHOD_TYPE_BASIC"
        })).expect("camelCase CreateOidcApp");
        assert_eq!(b.name, "Chat");
        assert_eq!(b.redirect_uris, vec!["https://x/cb".to_string()]);
        assert_eq!(b.app_type, "OIDC_APP_TYPE_WEB");
        assert_eq!(b.auth_method_type, "OIDC_AUTH_METHOD_TYPE_BASIC");
    }

    #[test]
    fn update_oidc_config_accepts_camelcase() {
        let b: UpdateOidcConfig = serde_json::from_value(json!({
            "redirectUris": ["https://x/cb"],
            "responseTypes": ["OIDC_RESPONSE_TYPE_CODE"],
            "grantTypes": ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
            "appType": "OIDC_APP_TYPE_NATIVE",
            "authMethodType": "OIDC_AUTH_METHOD_TYPE_NONE"
        })).expect("camelCase UpdateOidcConfig");
        assert_eq!(b.app_type, "OIDC_APP_TYPE_NATIVE");
        assert_eq!(b.response_types, vec!["OIDC_RESPONSE_TYPE_CODE".to_string()]);
    }
```

- [ ] **Step 2: Run it, expect FAIL** — `cargo test -p llm-chat-admin-api create_oidc_app_accepts_camelcase` → `error[E0412]: cannot find type \`CreateOidcApp\` in this scope` (struct + handlers not added).
- [ ] **Step 3: Implement** — in `admin-api/src/api/mod.rs`, add the six routes to the router (insert after the `/api/roles` route line, before `.with_state(state)`):

```rust
        .route("/api/apps", get(list_apps).post(create_oidc_app))
        .route("/api/apps/{appId}", get(get_app).put(update_oidc_config).delete(delete_app))
        .route("/api/apps/{appId}/secret", post(regenerate_app_secret))
```

Then add the handlers + contract structs (place them after the `delete_secret` handler, before the `contract_tests` module):

```rust
async fn list_apps(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "result": st.zitadel.list_apps().await? })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateOidcApp {
    name: String,
    redirect_uris: Vec<String>,
    response_types: Vec<String>,
    grant_types: Vec<String>,
    app_type: String,
    auth_method_type: String,
}
// clientSecret (WEB+BASIC) returned ONCE; streamed straight to the operator,
// never persisted/logged server-side (design §3 secret invariant).
async fn create_oidc_app(_op: Operator, State(st): State<AppState>, Json(b): Json<CreateOidcApp>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.create_oidc_app(
        &b.name, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?))
}

async fn get_app(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_app(&app_id).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateOidcConfig {
    redirect_uris: Vec<String>,
    response_types: Vec<String>,
    grant_types: Vec<String>,
    app_type: String,
    auth_method_type: String,
}
async fn update_oidc_config(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>, Json(b): Json<UpdateOidcConfig>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.update_oidc_config(
        &app_id, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?;
    Ok(Json(json!({ "ok": true })))
}

// clientSecret returned ONCE on regenerate; streamed straight through (design §3).
async fn regenerate_app_secret(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.regenerate_app_secret(&app_id).await?))
}

async fn delete_app(_op: Operator, State(st): State<AppState>, Path(app_id): Path<String>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_app(&app_id).await?;
    Ok(Json(json!({ "ok": true })))
}
```

- [ ] **Step 4: Run it, expect PASS** — `cargo test -p llm-chat-admin-api create_oidc_app_accepts_camelcase update_oidc_config_accepts_camelcase` → both pass. Then `cargo build -p llm-chat-admin-api` to confirm the router wiring compiles.
- [ ] **Step 5: Commit** —
```
git add admin-api/src/api/mod.rs
git commit -m "feat(admin-api): /api/apps* routes — OIDC app CRUD behind Operator (§8)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2.4: Frontend types + the shared OIDC mapper (`lib/types.ts`, `lib/oidc.ts`)

DRY: both the create dialog and the edit dialog need the same enum option lists and the same form↔body mapping. Put them in one pure, unit-tested `lib/oidc.ts`; mirror the existing `lib/api.test.ts` pattern for the unit test.

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/oidc.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { appToConfigForm, formToConfigBody, APP_TYPES, AUTH_METHODS } from "../lib/oidc";
import type { OidcApp } from "../lib/types";

const app: OidcApp = {
  id: "a1",
  name: "Chat",
  oidcConfig: {
    clientId: "c1",
    redirectUris: ["https://x/cb", "https://x/cb2"],
    responseTypes: ["OIDC_RESPONSE_TYPE_CODE"],
    grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
    appType: "OIDC_APP_TYPE_WEB",
    authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC",
  },
};

describe("oidc mapper", () => {
  it("appToConfigForm flattens uris/types to newline + comma strings", () => {
    const f = appToConfigForm(app);
    expect(f.redirectUris).toBe("https://x/cb\nhttps://x/cb2");
    expect(f.appType).toBe("OIDC_APP_TYPE_WEB");
    expect(f.authMethodType).toBe("OIDC_AUTH_METHOD_TYPE_BASIC");
    expect(f.grantTypes).toEqual(
      ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
    );
  });

  it("formToConfigBody splits + trims + drops blank redirect lines", () => {
    const body = formToConfigBody({
      name: "Chat",
      redirectUris: "  https://x/cb \n\n https://x/cb2 \n",
      responseTypes: ["OIDC_RESPONSE_TYPE_CODE"],
      grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
      appType: "OIDC_APP_TYPE_NATIVE",
      authMethodType: "OIDC_AUTH_METHOD_TYPE_NONE",
    });
    expect(body.redirectUris).toEqual(["https://x/cb", "https://x/cb2"]);
    expect(body.appType).toBe("OIDC_APP_TYPE_NATIVE");
  });

  it("exposes the enum option lists", () => {
    expect(APP_TYPES.map((o) => o.value)).toContain("OIDC_APP_TYPE_WEB");
    expect(AUTH_METHODS.map((o) => o.value)).toContain("OIDC_AUTH_METHOD_TYPE_NONE");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `npm run test -- oidc` (in `admin-web`) → `Error: Failed to load url ../lib/oidc` (file doesn't exist).
- [ ] **Step 3: Implement** — add to `admin-web/lib/types.ts`:

```ts
export type OidcAppType = "OIDC_APP_TYPE_WEB" | "OIDC_APP_TYPE_NATIVE" | "OIDC_APP_TYPE_USER_AGENT";
export type OidcAuthMethod =
  | "OIDC_AUTH_METHOD_TYPE_BASIC" | "OIDC_AUTH_METHOD_TYPE_POST"
  | "OIDC_AUTH_METHOD_TYPE_NONE" | "OIDC_AUTH_METHOD_TYPE_PRIVATE_KEY_JWT";
export type OidcResponseType = "OIDC_RESPONSE_TYPE_CODE" | "OIDC_RESPONSE_TYPE_ID_TOKEN" | "OIDC_RESPONSE_TYPE_ID_TOKEN_TOKEN";
export type OidcGrantType =
  | "OIDC_GRANT_TYPE_AUTHORIZATION_CODE" | "OIDC_GRANT_TYPE_IMPLICIT"
  | "OIDC_GRANT_TYPE_REFRESH_TOKEN" | "OIDC_GRANT_TYPE_DEVICE_CODE" | "OIDC_GRANT_TYPE_TOKEN_EXCHANGE";

export interface OidcConfig {
  clientId?: string;
  redirectUris?: string[];
  responseTypes?: OidcResponseType[];
  grantTypes?: OidcGrantType[];
  appType?: OidcAppType;
  authMethodType?: OidcAuthMethod;
}

export interface OidcApp {
  id: string;
  name: string;
  state?: string;
  oidcConfig?: OidcConfig;
}

export interface OidcAppList {
  result: OidcApp[];
}

export interface CreateOidcAppInput {
  name: string;
  redirectUris: string[];
  responseTypes: OidcResponseType[];
  grantTypes: OidcGrantType[];
  appType: OidcAppType;
  authMethodType: OidcAuthMethod;
}

// Returned ONCE on create + secret regenerate; never readable again.
export interface AppSecret {
  appId?: string;
  clientId?: string;
  clientSecret: string;
}
```

Then create `admin-web/lib/oidc.ts`:

```ts
import type {
  OidcApp, OidcAppType, OidcAuthMethod, OidcGrantType, OidcResponseType,
  CreateOidcAppInput,
} from "@/lib/types";

export interface ConfigForm {
  name: string;
  redirectUris: string;            // newline-separated in the textarea
  responseTypes: OidcResponseType[];
  grantTypes: OidcGrantType[];
  appType: OidcAppType;
  authMethodType: OidcAuthMethod;
}

export const APP_TYPES: { value: OidcAppType; label: string }[] = [
  { value: "OIDC_APP_TYPE_WEB", label: "Web (confidential)" },
  { value: "OIDC_APP_TYPE_NATIVE", label: "Native (PKCE, public)" },
  { value: "OIDC_APP_TYPE_USER_AGENT", label: "User-agent (SPA)" },
];

export const AUTH_METHODS: { value: OidcAuthMethod; label: string }[] = [
  { value: "OIDC_AUTH_METHOD_TYPE_BASIC", label: "Basic (client secret)" },
  { value: "OIDC_AUTH_METHOD_TYPE_POST", label: "POST (client secret)" },
  { value: "OIDC_AUTH_METHOD_TYPE_NONE", label: "None (PKCE only)" },
  { value: "OIDC_AUTH_METHOD_TYPE_PRIVATE_KEY_JWT", label: "Private key JWT" },
];

export const RESPONSE_TYPES: { value: OidcResponseType; label: string }[] = [
  { value: "OIDC_RESPONSE_TYPE_CODE", label: "code" },
  { value: "OIDC_RESPONSE_TYPE_ID_TOKEN", label: "id_token" },
  { value: "OIDC_RESPONSE_TYPE_ID_TOKEN_TOKEN", label: "id_token token" },
];

export const GRANT_TYPES: { value: OidcGrantType; label: string }[] = [
  { value: "OIDC_GRANT_TYPE_AUTHORIZATION_CODE", label: "authorization_code" },
  { value: "OIDC_GRANT_TYPE_REFRESH_TOKEN", label: "refresh_token" },
  { value: "OIDC_GRANT_TYPE_IMPLICIT", label: "implicit" },
  { value: "OIDC_GRANT_TYPE_DEVICE_CODE", label: "device_code" },
  { value: "OIDC_GRANT_TYPE_TOKEN_EXCHANGE", label: "token_exchange" },
];

/// Flatten an app's oidcConfig into the editable form shape (read side of RMW).
export function appToConfigForm(app: OidcApp): ConfigForm {
  const c = app.oidcConfig ?? {};
  return {
    name: app.name,
    redirectUris: (c.redirectUris ?? []).join("\n"),
    responseTypes: c.responseTypes ?? ["OIDC_RESPONSE_TYPE_CODE"],
    grantTypes: c.grantTypes ?? ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
    appType: c.appType ?? "OIDC_APP_TYPE_WEB",
    authMethodType: c.authMethodType ?? "OIDC_AUTH_METHOD_TYPE_BASIC",
  };
}

/// Build the create/update body from the form (write side of RMW).
export function formToConfigBody(f: ConfigForm): CreateOidcAppInput {
  return {
    name: f.name,
    redirectUris: f.redirectUris
      .split("\n").map((s) => s.trim()).filter((s) => s.length > 0),
    responseTypes: f.responseTypes,
    grantTypes: f.grantTypes,
    appType: f.appType,
    authMethodType: f.authMethodType,
  };
}
```

- [ ] **Step 4: Run it, expect PASS** — `npm run test -- oidc` → 3 passing.
- [ ] **Step 5: Commit** —
```
git add admin-web/lib/types.ts admin-web/lib/oidc.ts admin-web/__tests__/oidc.test.ts
git commit -m "feat(admin-web): OIDC app types + shared config mapper (§8)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2.5: Apps columns + secret-reveal dialog

Mirrors `components/users/columns.tsx` (DataTable columns + a per-row `DropdownMenu`) and adds the one-time secret reveal `Dialog`. The reveal dialog only shows what the caller passes (the pass-through secret) and never refetches — once dismissed, it is gone (the invariant). Test the columns with the existing `columns.test.tsx` pattern.

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/apps-columns.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { flexRender } from "@tanstack/react-table";
import { buildAppColumns } from "../components/apps/columns";
import type { OidcApp } from "../lib/types";

const app: OidcApp = {
  id: "a1", name: "Chat",
  oidcConfig: { clientId: "c1", appType: "OIDC_APP_TYPE_WEB", authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC" },
};

describe("app columns", () => {
  it("has name, clientId, appType, actions columns", () => {
    const ids = buildAppColumns({ onEdit: vi.fn(), onRotate: vi.fn(), onDelete: vi.fn() })
      .map((c) => ("accessorKey" in c ? (c as any).accessorKey : c.id));
    expect(ids).toEqual(expect.arrayContaining(["name", "clientId", "appType", "actions"]));
  });

  it("fires onRotate from the row action menu", async () => {
    const onRotate = vi.fn();
    const cols = buildAppColumns({ onEdit: vi.fn(), onRotate, onDelete: vi.fn() });
    const actions = cols.find((c) => c.id === "actions")!;
    const ctx = { row: { original: app } };
    render(<>{flexRender((actions as any).cell, ctx as any)}</>);
    const trigger = screen.getByRole("button");
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.click(trigger);
    const item = await screen.findByTestId("action-rotate-secret");
    item.click();
    expect(onRotate).toHaveBeenCalledWith(app);
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `npm run test -- apps-columns` → `Failed to load url ../components/apps/columns` (file missing).
- [ ] **Step 3: Implement** — create `admin-web/components/apps/columns.tsx`:

```tsx
"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { MoreHorizontal } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { OidcApp } from "@/lib/types";

export interface AppColumnHandlers {
  onEdit: (a: OidcApp) => void;
  onRotate: (a: OidcApp) => void;
  onDelete: (a: OidcApp) => void;
}

export function buildAppColumns(h: AppColumnHandlers): ColumnDef<OidcApp>[] {
  return [
    { accessorKey: "name", header: "Name" },
    {
      accessorKey: "clientId", header: "Client ID",
      cell: ({ row }) => (
        <code className="text-xs">{row.original.oidcConfig?.clientId ?? "—"}</code>
      ),
    },
    {
      accessorKey: "appType", header: "Type",
      cell: ({ row }) => (
        <Badge variant="secondary">
          {(row.original.oidcConfig?.appType ?? "").replace("OIDC_APP_TYPE_", "")}
        </Badge>
      ),
    },
    {
      id: "actions",
      cell: ({ row }) => {
        const a = row.original;
        return (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" className="h-8 w-8 p-0">
                <span className="sr-only">Open menu</span>
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuLabel>Actions</DropdownMenuLabel>
              <DropdownMenuItem data-testid="action-edit" onSelect={() => h.onEdit(a)}>
                Edit config
              </DropdownMenuItem>
              <DropdownMenuItem data-testid="action-rotate-secret" onSelect={() => h.onRotate(a)}>
                Rotate client secret
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem data-testid="action-delete"
                className="text-destructive" onSelect={() => h.onDelete(a)}>
                Delete app
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        );
      },
    },
  ];
}
```

Then create `admin-web/components/apps/secret-reveal-dialog.tsx`:

```tsx
"use client";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

// One-time secret reveal (design §3 invariant). The secret is held only in the
// caller's state from the create/regenerate response and is NEVER refetched;
// dismissing this dialog discards it. No logging.
export function SecretRevealDialog({
  clientId, clientSecret, onClose,
}: {
  clientId?: string;
  clientSecret: string | null;
  onClose: () => void;
}) {
  async function copy() {
    try {
      await navigator.clipboard.writeText(clientSecret ?? "");
      toast.success("Copied to clipboard");
    } catch {
      toast.error("Copy failed — select and copy manually");
    }
  }
  return (
    <Dialog open={!!clientSecret} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Client secret — copy it now</DialogTitle>
          <DialogDescription>
            This secret is shown once and cannot be retrieved again. Copy and
            store it before closing this dialog.
          </DialogDescription>
        </DialogHeader>
        {clientId && (
          <div className="space-y-1">
            <p className="text-sm text-muted-foreground">Client ID</p>
            <Input readOnly value={clientId} data-testid="reveal-client-id" />
          </div>
        )}
        <div className="space-y-1">
          <p className="text-sm text-muted-foreground">Client secret</p>
          <Input readOnly value={clientSecret ?? ""} data-testid="reveal-client-secret" />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={copy} data-testid="reveal-copy">Copy</Button>
          <Button onClick={onClose} data-testid="reveal-done">Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 4: Run it, expect PASS** — `npm run test -- apps-columns` → 2 passing.
- [ ] **Step 5: Commit** —
```
git add admin-web/components/apps/columns.tsx admin-web/components/apps/secret-reveal-dialog.tsx admin-web/__tests__/apps-columns.test.tsx
git commit -m "feat(admin-web): apps columns + one-time secret reveal dialog (§8)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2.6: App create/edit form dialog + the Applications list page

The form dialog handles both create (`POST /api/apps`) and edit (read-modify-write `PUT /api/apps/{id}`). On create with a WEB+BASIC method, the response carries `clientSecret` — the page hands it to the `SecretRevealDialog`. The list page mirrors `users/page.tsx` exactly (`useCallback load()`, mount + post-mutation reload, `toast.error(e instanceof ApiError ? e.message : 'fallback')`, 401 swallowed). Delete + rotate use `ConfirmDialog` (reuse `components/users/confirm-dialog.tsx`).

- [ ] **Step 1: Write the failing test** — create `admin-web/__tests__/applications-page.test.tsx` (smoke render + load, mirroring how `api.test.ts` stubs fetch; asserts the page lists apps and shows the create button):

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import ApplicationsPage from "../app/(dash)/applications/page";

function mockJson(body: unknown) {
  return { ok: true, status: 200, json: async () => body, text: async () => JSON.stringify(body), headers: new Headers() } as unknown as Response;
}

beforeEach(() => {
  Object.defineProperty(window, "location", { value: { assign: vi.fn(), href: "" }, writable: true });
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (url.startsWith("/api/me")) return mockJson({ userId: "o1", name: "Op", roles: ["chat.admin"] });
    if (url.startsWith("/api/apps")) return mockJson({ result: [
      { id: "a1", name: "Chat", oidcConfig: { clientId: "c1", appType: "OIDC_APP_TYPE_WEB" } },
    ] });
    return mockJson({});
  }));
});
afterEach(() => vi.restoreAllMocks());

describe("ApplicationsPage", () => {
  it("loads and lists apps + shows create button", async () => {
    render(<ApplicationsPage />);
    await waitFor(() => expect(screen.getByText("Chat")).toBeInTheDocument());
    expect(screen.getByTestId("create-app")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL** — `npm run test -- applications-page` → `Failed to load url ../app/(dash)/applications/page` (page + form dialog missing).
- [ ] **Step 3: Implement** — create `admin-web/components/apps/app-form-dialog.tsx`:

```tsx
"use client";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger, DialogFooter,
} from "@/components/ui/dialog";
import {
  Form, FormControl, FormField, FormItem, FormLabel, FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { api, ApiError } from "@/lib/api";
import type { OidcApp, AppSecret } from "@/lib/types";
import {
  APP_TYPES, AUTH_METHODS, appToConfigForm, formToConfigBody, type ConfigForm,
} from "@/lib/oidc";

const schema = z.object({
  name: z.string().min(1),
  redirectUris: z.string().min(1, "at least one redirect URI"),
  appType: z.enum(["OIDC_APP_TYPE_WEB", "OIDC_APP_TYPE_NATIVE", "OIDC_APP_TYPE_USER_AGENT"]),
  authMethodType: z.enum([
    "OIDC_AUTH_METHOD_TYPE_BASIC", "OIDC_AUTH_METHOD_TYPE_POST",
    "OIDC_AUTH_METHOD_TYPE_NONE", "OIDC_AUTH_METHOD_TYPE_PRIVATE_KEY_JWT",
  ]),
});
type FormShape = z.infer<typeof schema>;

const DEFAULTS = {
  responseTypes: ["OIDC_RESPONSE_TYPE_CODE"] as const,
  grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"] as const,
};

// `app` null => create mode; non-null => edit (read-modify-write the full config).
export function AppFormDialog({
  app, open, onOpenChange, onSaved, onSecret,
}: {
  app: OidcApp | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
  onSecret: (s: AppSecret) => void;
}) {
  const isEdit = !!app;
  const form = useForm<FormShape>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: "", redirectUris: "",
      appType: "OIDC_APP_TYPE_WEB", authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC",
    },
  });
  useEffect(() => {
    if (app) {
      const f = appToConfigForm(app);
      form.reset({ name: f.name, redirectUris: f.redirectUris, appType: f.appType, authMethodType: f.authMethodType });
    } else {
      form.reset({ name: "", redirectUris: "", appType: "OIDC_APP_TYPE_WEB", authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC" });
    }
  }, [app, form]);

  async function onSubmit(values: FormShape) {
    const cfg: ConfigForm = {
      name: values.name,
      redirectUris: values.redirectUris,
      responseTypes: [...DEFAULTS.responseTypes],
      grantTypes: [...DEFAULTS.grantTypes],
      appType: values.appType,
      authMethodType: values.authMethodType,
    };
    const body = formToConfigBody(cfg);
    try {
      if (isEdit && app) {
        // read-modify-write: PUT the whole oidc_config (design §8).
        await api.put(`/api/apps/${app.id}`, {
          redirectUris: body.redirectUris,
          responseTypes: body.responseTypes,
          grantTypes: body.grantTypes,
          appType: body.appType,
          authMethodType: body.authMethodType,
        });
        toast.success("App updated");
      } else {
        const created = await api.post<AppSecret>("/api/apps", body);
        toast.success("App created");
        if (created?.clientSecret) onSecret(created); // one-time reveal
      }
      onOpenChange(false);
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Save failed");
    }
  }

  const inner = (
    <DialogContent>
      <DialogHeader><DialogTitle>{isEdit ? "Edit application" : "Create application"}</DialogTitle></DialogHeader>
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
          <FormField control={form.control} name="name" render={({ field }) => (
            <FormItem><FormLabel>Name</FormLabel>
              <FormControl><Input {...field} disabled={isEdit} /></FormControl><FormMessage /></FormItem>
          )} />
          <FormField control={form.control} name="redirectUris" render={({ field }) => (
            <FormItem><FormLabel>Redirect URIs (one per line)</FormLabel>
              <FormControl>
                <textarea
                  className="flex min-h-24 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm"
                  {...field}
                />
              </FormControl><FormMessage /></FormItem>
          )} />
          <FormField control={form.control} name="appType" render={({ field }) => (
            <FormItem><FormLabel>App type</FormLabel>
              <Select onValueChange={field.onChange} value={field.value}>
                <FormControl><SelectTrigger><SelectValue /></SelectTrigger></FormControl>
                <SelectContent>
                  {APP_TYPES.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
                </SelectContent>
              </Select><FormMessage /></FormItem>
          )} />
          <FormField control={form.control} name="authMethodType" render={({ field }) => (
            <FormItem><FormLabel>Auth method</FormLabel>
              <Select onValueChange={field.onChange} value={field.value}>
                <FormControl><SelectTrigger><SelectValue /></SelectTrigger></FormControl>
                <SelectContent>
                  {AUTH_METHODS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
                </SelectContent>
              </Select><FormMessage /></FormItem>
          )} />
          <DialogFooter><Button type="submit">{isEdit ? "Save" : "Create"}</Button></DialogFooter>
        </form>
      </Form>
    </DialogContent>
  );

  // create mode owns its trigger button; edit mode is controlled by the page.
  if (isEdit) {
    return <Dialog open={open} onOpenChange={onOpenChange}>{inner}</Dialog>;
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button data-testid="create-app">Create application</Button>
      </DialogTrigger>
      {inner}
    </Dialog>
  );
}
```

Then create `admin-web/app/(dash)/applications/page.tsx`:

```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { buildAppColumns } from "@/components/apps/columns";
import { AppFormDialog } from "@/components/apps/app-form-dialog";
import { SecretRevealDialog } from "@/components/apps/secret-reveal-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { api, ApiError } from "@/lib/api";
import type { OidcApp, OidcAppList, AppSecret } from "@/lib/types";

export default function ApplicationsPage() {
  const [apps, setApps] = useState<OidcApp[]>([]);
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<OidcApp | null>(null);
  const [rotateTarget, setRotateTarget] = useState<OidcApp | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<OidcApp | null>(null);
  const [revealed, setRevealed] = useState<AppSecret | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.get<OidcAppList>("/api/apps");
      setApps(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load applications");
      }
    }
  }, []);

  useEffect(() => {
    api.get("/api/me").catch(() => {});
    load();
  }, [load]);

  async function confirmRotate() {
    if (!rotateTarget) return;
    try {
      const s = await api.post<AppSecret>(`/api/apps/${rotateTarget.id}/secret`);
      toast.success("Secret rotated");
      if (s?.clientSecret) setRevealed(s); // one-time reveal
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Rotate failed");
    } finally {
      setRotateTarget(null);
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      await api.del(`/api/apps/${deleteTarget.id}`);
      toast.success("Application deleted");
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    } finally {
      setDeleteTarget(null);
      load();
    }
  }

  const columns = buildAppColumns({
    onEdit: setEditTarget,
    onRotate: setRotateTarget,
    onDelete: setDeleteTarget,
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Applications</h1>
        <AppFormDialog app={null} open={createOpen} onOpenChange={setCreateOpen}
          onSaved={load} onSecret={setRevealed} />
      </div>
      <DataTable columns={columns} data={apps}
        filterColumn="name" filterPlaceholder="Filter by name..."
        emptyMessage="No applications." />
      <AppFormDialog app={editTarget} open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)} onSaved={load} onSecret={setRevealed} />
      <SecretRevealDialog clientId={revealed?.clientId}
        clientSecret={revealed?.clientSecret ?? null} onClose={() => setRevealed(null)} />
      <ConfirmDialog open={!!rotateTarget}
        onOpenChange={(o) => !o && setRotateTarget(null)}
        title="Rotate client secret?"
        description="A new secret is generated and shown once. Any client still using the old secret will immediately fail authentication until updated."
        confirmLabel="Rotate" onConfirm={confirmRotate} />
      <ConfirmDialog open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete application?"
        description="This removes the OIDC client. Changing or removing redirectUris can instantly break a live login for users mid-flow. This cannot be undone."
        confirmLabel="Delete" onConfirm={confirmDelete} />
    </div>
  );
}
```

Add the nav entry (the spec mandates a single source of nav truth — append one `NAV` entry; `AppWindow` is the spec-named icon). In `admin-web/components/shell/Sidebar.tsx` (created in Phase 0), add to the `NAV` array, after the Roles entry:

```tsx
  { icon: AppWindow, label: "Applications", href: "/applications", match: "/applications" },
```

and ensure `AppWindow` is imported from `lucide-react` in that file's icon import (Phase 0 already imports the §2 nav icon set including `AppWindow`).

- [ ] **Step 4: Run it, expect PASS** — `npm run test -- applications-page` → passing. Then `npm run build` to confirm the route-group page typechecks under Next.js 16 (no async-params needed: this is a static client page, no `[appId]` segment — edit/rotate are dialogs, not routes).
- [ ] **Step 5: Commit** —
```
git add admin-web/components/apps/app-form-dialog.tsx "admin-web/app/(dash)/applications/page.tsx" admin-web/components/shell/Sidebar.tsx admin-web/__tests__/applications-page.test.tsx
git commit -m "feat(admin-web): Applications page — create/edit/rotate/delete + nav (§8)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2.7: e2e secret-reveal-once Playwright assertion + apps lifecycle integration test

Two verifications: (a) extend `e2e/smoke.spec.ts` with the App-create flow that asserts the secret is revealed **exactly once** (visible after create, gone after dismiss, and not refetchable on the page), reusing the existing `ADMIN_IT=1` operator login; (b) the Rust apps lifecycle is already covered by Task 2.1's `it_verify_oidc_config_put_and_secret_regen` (create→get→update→regen→delete) — no second integration test needed (DRY).

- [ ] **Step 1: Write the failing test** — append to `admin-web/e2e/smoke.spec.ts`, inside the `test.describe("authenticated operator flow", …)` block (after the existing create-machine-user test):

```ts
  test("create OIDC app reveals the client secret exactly once", async ({ page }) => {
    await page.goto("/login");
    await page.locator('input[name="loginName"]').fill(process.env.ADMIN_IT_USER!);
    await page.getByRole("button", { name: /next|continue/i }).click();
    await page.locator('input[name="password"]').fill(process.env.ADMIN_IT_PASS!);
    await page.getByRole("button", { name: /next|continue|sign in/i }).click();
    const skip2fa = page.getByRole("button", { name: /skip/i });
    await skip2fa.waitFor({ state: "visible", timeout: 8000 })
      .then(() => skip2fa.click()).catch(() => {});

    await page.goto("/applications");
    await expect(page.getByRole("heading", { name: "Applications" })).toBeVisible();

    const appName = `pw-app-${Date.now()}`;
    await page.getByTestId("create-app").click();
    await page.getByLabel("Name").fill(appName);
    await page.getByLabel(/redirect uris/i).fill("https://example.localhost/callback");
    // appType defaults to Web (confidential) + Basic -> server returns a secret.
    await page.getByRole("button", { name: "Create" }).click();

    // the secret is revealed once, with a copy affordance.
    const secret = page.getByTestId("reveal-client-secret");
    await expect(secret).toBeVisible();
    const secretValue = await secret.inputValue();
    expect(secretValue.length).toBeGreaterThan(0);
    await expect(page.getByText(/shown once and cannot be retrieved again/i)).toBeVisible();

    // dismiss -> the secret is gone and NOT recoverable from the list page.
    await page.getByTestId("reveal-done").click();
    await expect(page.getByTestId("reveal-client-secret")).toHaveCount(0);
    await page.getByPlaceholder(/filter by name/i).fill(appName);
    await expect(page.getByText(appName)).toBeVisible();
    // the row shows clientId but never the secret value.
    await expect(page.getByText(secretValue)).toHaveCount(0);
  });
```

- [ ] **Step 2: Run it, expect FAIL** — without the stack the test is `test.skip(!FULL, …)` and reports skipped; with `ADMIN_IT=1` but before the page is deployed it fails at `getByRole("heading", { name: "Applications" })`. Run: `cd admin-web && set ADMIN_IT=1 && npm run e2e -- -g "reveals the client secret exactly once"` → expect FAIL (heading/route not served) until the page is built and running.
- [ ] **Step 3: Implement** — no new app code; this task validates Tasks 2.5–2.6 end-to-end. If the assertion exposes a defect (e.g. the secret persists in the DOM after dismiss, or the list page re-renders it), fix the offending component (`SecretRevealDialog` must clear on `onClose`; the page must set `revealed` to `null`) — root-cause, never weaken the assertion.
- [ ] **Step 4: Run it, expect PASS** — bring up the stack (`docker compose up` per `config.md`), then `cd admin-web && set ADMIN_IT=1 && set ADMIN_IT_USER=<op> && set ADMIN_IT_PASS=<pw> && npm run e2e -- -g "reveals the client secret exactly once"` → passing. Also re-run the full Rust suite `cargo test -p llm-chat-admin-api` (offline unit tests) and, with the live env, `cargo test -p llm-chat-admin-api --test integration it_verify_oidc_config_put_and_secret_regen` → all green.
- [ ] **Step 5: Commit** —
```
git add admin-web/e2e/smoke.spec.ts
git commit -m "test(admin-web): e2e secret-reveal-once assertion for OIDC app create (§13)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

## Phase 3: Project & Org Settings

Implements spec §9. Backend adds two new `ZitadelClient` modules — `zitadel/project.rs` (get/update the project) and `zitadel/policies.rs` (login / password-complexity / lockout get + **upsert**) — plus 8 thin `Operator`-gated routes in `api/mod.rs`. The upsert trap (org on the *default* policy → `PUT` 404 → fall back to `POST` add-custom) is nailed by an `ADMIN_IT` live test that asserts the **actual** error variant. The login-policy `mfaInitSkipLifetime` control (the 2FA-setup-nudge fix from §3/§9) is surfaced, and protobuf `Duration` strings (`"0s"`) are passed through untouched. Frontend adds `app/(dash)/project/page.tsx` — four cards (Project, Login policy, Password complexity, Lockout) with `switch`/`input`/`select` edit forms mirroring `users/page.tsx` exactly — one NAV entry, and a policy-edit Playwright spec.

**Depends on Phase 0:** `components/ui/card.tsx`, `components/ui/switch.tsx`, the `emptyMessage` `DataTable` prop, the `(dash)/layout.tsx` shell, `components/shell/nav.ts` (the typed `NAV` array), and the **ORG_OWNER SA bump** (without it every policy/project write 403s). Do not start Phase 3 until Phase 0 is green.

**Files:**

- `admin-api/src/zitadel/project.rs` — **Create.** `get_project`/`update_project` over `GET/PUT /management/v1/projects/{id}`, `Value` passthrough.
- `admin-api/src/zitadel/policies.rs` — **Create.** `get_/upsert_login_policy`, `get_/upsert_password_complexity_policy`, `get_/upsert_lockout_policy`; the PUT→`NotFound`→POST upsert branch lives here once and is reused.
- `admin-api/src/zitadel/mod.rs` — **Modify.** Add `pub mod policies;` + `pub mod project;`.
- `admin-api/src/api/mod.rs` — **Modify.** 8 routes (`GET/PUT /api/project`, `GET/PUT /api/org/policies/{login,password-complexity,lockout}`) + handlers + camelCase contract tests.
- `admin-api/tests/integration.rs` — **Modify.** `ADMIN_IT` test that drives the real upsert and asserts the `NotFound`-vs-`Invalid` branch on a default policy.
- `admin-web/lib/types.ts` — **Modify.** `Project`, `LoginPolicy`, `PasswordComplexityPolicy`, `LockoutPolicy` interfaces (camelCase, `Duration` as `string`).
- `admin-web/components/project/policy-cards.tsx` — **Create.** The four `card` edit forms (`ProjectCard`, `LoginPolicyCard`, `PasswordComplexityCard`, `LockoutCard`), each react-hook-form + `zodResolver`.
- `admin-web/app/(dash)/project/page.tsx` — **Create.** Thin client page: `load()` fans out the four GETs into state, renders the four cards, reloads after each save. Mirrors `users/page.tsx`.
- `admin-web/components/shell/nav.ts` — **Modify.** Append the `{ icon: Building2, label: "Project", href: "/project", match: "/project" }` NAV entry.
- `admin-web/__tests__/policy-cards.test.tsx` — **Create.** Vitest: a Duration-string round-trips through the login-policy form without mangling.
- `admin-web/e2e/smoke.spec.ts` — **Modify.** `ADMIN_IT` Playwright: navigate to Project, toggle a login-policy switch, save, assert the success toast.

---

### Task 3.1: `zitadel/project.rs` — get/update the project

- [ ] **Step 1: Write the failing test.** Append to a new `#[cfg(test)] mod tests` at the bottom of `admin-api/src/zitadel/project.rs` (the module won't compile yet — that's the failing state). Create the file with ONLY this so it fails to find the impl:

```rust
//! Project read/update wrappers: GET/PUT /management/v1/projects/{id} (design §9).
//! Pass the Zitadel JSON through untouched (camelCase preserved); the project id
//! is always cfg.project_id (the single "Chat" app), never client-supplied.

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

#[cfg(test)]
mod tests {
    use super::*;

    // PURE shape test: the update body we send must carry the editable project
    // fields under their exact v1 keys (verified against provision.py create_project).
    #[test]
    fn update_project_body_uses_v1_keys() {
        let body = super::ZitadelClient::project_update_body(
            "Chat", true, false, true,
        );
        assert_eq!(body["name"], json!("Chat"));
        assert_eq!(body["projectRoleAssertion"], json!(true));
        assert_eq!(body["projectRoleCheck"], json!(false));
        assert_eq!(body["hasProjectCheck"], json!(true));
    }
}
```

- [ ] **Step 2: Run it, expect FAIL.** `cargo test -p llm-chat-admin-api --lib zitadel::project` — expect `no function or associated item named 'project_update_body' found`.

- [ ] **Step 3: Implement.** Insert the impl block above the test module in `admin-api/src/zitadel/project.rs`:

```rust
impl ZitadelClient {
    /// PURE: the editable-fields body for PUT /projects/{id}. Keys verified
    /// against provision.py create_project (name + the three role/project flags).
    pub fn project_update_body(
        name: &str,
        role_assertion: bool,
        role_check: bool,
        has_project_check: bool,
    ) -> Value {
        json!({
            "name": name,
            "projectRoleAssertion": role_assertion,
            "projectRoleCheck": role_check,
            "hasProjectCheck": has_project_check,
        })
    }

    /// GET /management/v1/projects/{id} -> {project:{...}} (camelCase passthrough).
    pub async fn get_project(&self) -> Result<Value, ZitadelError> {
        let url = format!(
            "{}/management/v1/projects/{}",
            self.cfg.issuer, self.cfg.project_id
        );
        let v = self.get_json(&url).await?;
        // Unwrap the {project:{...}} envelope when present; tolerate a bare object.
        Ok(v.get("project").cloned().unwrap_or(v))
    }

    /// PUT /management/v1/projects/{id} with the editable fields. Returns the
    /// Zitadel response (details) passed through.
    pub async fn update_project(
        &self,
        name: &str,
        role_assertion: bool,
        role_check: bool,
        has_project_check: bool,
    ) -> Result<Value, ZitadelError> {
        let url = format!(
            "{}/management/v1/projects/{}",
            self.cfg.issuer, self.cfg.project_id
        );
        let body =
            Self::project_update_body(name, role_assertion, role_check, has_project_check);
        self.put_json(&url, &body).await
    }
}
```

Then add the module to `admin-api/src/zitadel/mod.rs` — insert `pub mod project;` after `pub mod model;`:

```rust
pub mod model;
pub mod project;
```

- [ ] **Step 4: Run it, expect PASS.** `cargo test -p llm-chat-admin-api --lib zitadel::project` — expect `test result: ok. 1 passed`.

- [ ] **Step 5: Commit.**
```
git add admin-api/src/zitadel/project.rs admin-api/src/zitadel/mod.rs
git commit -m "feat(admin-api): project get/update wrappers (design §9)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.2: `zitadel/policies.rs` — the upsert branch (PUT then POST on NotFound)

- [ ] **Step 1: Write the failing test.** Create `admin-api/src/zitadel/policies.rs` with the header + a PURE test for the body builders and the upsert decision (no network), so it fails on the missing functions:

```rust
//! Org policy wrappers (design §9): login / password-complexity / lockout.
//! UPSERT TRAP: an org may still be on the instance *default* policy
//! (isDefault==true); a PUT then 404s because there is no custom policy to
//! update. We branch on the typed ZitadelError::NotFound and POST (add custom).
//! Protobuf Duration fields (e.g. mfaInitSkipLifetime) serialize as STRINGS
//! ("0s", "720h0m0s") — we pass them through untouched, never reparse.

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

/// Which Zitadel policy resource an upsert targets. The path suffix is the
/// single source of truth for both the GET, the PUT (update) and the POST (add).
#[derive(Clone, Copy)]
enum Policy {
    Login,
    PasswordComplexity,
    Lockout,
}

impl Policy {
    fn path(self) -> &'static str {
        match self {
            Policy::Login => "login",
            Policy::PasswordComplexity => "password/complexity",
            Policy::Lockout => "lockout",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_paths_match_zitadel_v1() {
        assert_eq!(Policy::Login.path(), "login");
        assert_eq!(Policy::PasswordComplexity.path(), "password/complexity");
        assert_eq!(Policy::Lockout.path(), "lockout");
    }

    // The login body must carry mfaInitSkipLifetime as the operator typed it —
    // a Duration STRING — not coerced to a number (design §9 protobuf Duration).
    #[test]
    fn login_body_preserves_duration_string() {
        let body = ZitadelClient::login_policy_body(&json!({
            "allowUsernamePassword": true,
            "forceMfa": false,
            "mfaInitSkipLifetime": "0s"
        }));
        assert_eq!(body["mfaInitSkipLifetime"], json!("0s"));
        assert_eq!(body["allowUsernamePassword"], json!(true));
    }

    // upsert_decision is pure: PUT result of NotFound means "fall back to POST",
    // any other error propagates, Ok means done.
    #[test]
    fn upsert_falls_back_only_on_not_found() {
        assert!(ZitadelClient::should_add_custom(&Err(ZitadelError::NotFound)));
        assert!(!ZitadelClient::should_add_custom(&Err(ZitadelError::Forbidden)));
        assert!(!ZitadelClient::should_add_custom(&Ok(json!({}))));
    }
}
```

- [ ] **Step 2: Run it, expect FAIL.** `cargo test -p llm-chat-admin-api --lib zitadel::policies` — expect `no function or associated item named 'login_policy_body' found` (and `should_add_custom`).

- [ ] **Step 3: Implement.** Insert above the test module in `admin-api/src/zitadel/policies.rs`:

```rust
impl ZitadelClient {
    /// PURE: pass the operator's login-policy fields through verbatim. We do NOT
    /// strip or recompute Duration strings (mfaInitSkipLifetime, "0s") — the form
    /// owns them and Zitadel round-trips them as protobuf Duration strings.
    pub fn login_policy_body(input: &Value) -> Value {
        input.clone()
    }

    /// PURE: the upsert fork. A custom-less org PUTs into thin air -> NOT_FOUND
    /// (mapped from Zitadel's 404 by error::map_status). Only then do we POST the
    /// add-custom variant; every other error is a real failure and must surface.
    pub fn should_add_custom(put_result: &Result<Value, ZitadelError>) -> bool {
        matches!(put_result, Err(ZitadelError::NotFound))
    }

    async fn get_policy(&self, p: Policy) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/policies/{}", self.cfg.issuer, p.path());
        // GET returns {policy:{...}}; unwrap when present, tolerate a bare object.
        let v = self.get_json(&url).await?;
        Ok(v.get("policy").cloned().unwrap_or(v))
    }

    /// Upsert a policy: PUT (update custom). On NOT_FOUND the org is still on the
    /// default policy, so POST the same body to add a custom one (design §9).
    async fn upsert_policy(&self, p: Policy, body: &Value) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/policies/{}", self.cfg.issuer, p.path());
        let put = self.put_json(&url, body).await;
        if Self::should_add_custom(&put) {
            return self.post_json(&url, body).await;
        }
        put
    }

    pub async fn get_login_policy(&self) -> Result<Value, ZitadelError> {
        self.get_policy(Policy::Login).await
    }
    pub async fn upsert_login_policy(&self, body: &Value) -> Result<Value, ZitadelError> {
        let b = Self::login_policy_body(body);
        self.upsert_policy(Policy::Login, &b).await
    }

    pub async fn get_password_complexity_policy(&self) -> Result<Value, ZitadelError> {
        self.get_policy(Policy::PasswordComplexity).await
    }
    pub async fn upsert_password_complexity_policy(
        &self,
        body: &Value,
    ) -> Result<Value, ZitadelError> {
        self.upsert_policy(Policy::PasswordComplexity, body).await
    }

    pub async fn get_lockout_policy(&self) -> Result<Value, ZitadelError> {
        self.get_policy(Policy::Lockout).await
    }
    pub async fn upsert_lockout_policy(&self, body: &Value) -> Result<Value, ZitadelError> {
        self.upsert_policy(Policy::Lockout, body).await
    }
}
```

Then register it in `admin-api/src/zitadel/mod.rs` — add `pub mod policies;` (alphabetical, after `pub mod model;`):

```rust
pub mod model;
pub mod policies;
pub mod project;
```

- [ ] **Step 4: Run it, expect PASS.** `cargo test -p llm-chat-admin-api --lib zitadel::policies` — expect `test result: ok. 3 passed`.

- [ ] **Step 5: Commit.**
```
git add admin-api/src/zitadel/policies.rs admin-api/src/zitadel/mod.rs
git commit -m "feat(admin-api): org policy upsert (PUT then add-custom on NotFound, design §9)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.3: `api/mod.rs` — 8 Operator-gated routes + camelCase contract tests

- [ ] **Step 1: Write the failing test.** Add to the existing `mod contract_tests` block at the bottom of `admin-api/src/api/mod.rs` (after `add_grant_accepts_rolekeys`):

```rust
    #[test]
    fn update_project_accepts_camelcase() {
        let b: UpdateProject = serde_json::from_value(json!({
            "name": "Chat", "projectRoleAssertion": true,
            "projectRoleCheck": false, "hasProjectCheck": true
        })).expect("camelCase UpdateProject");
        assert_eq!(b.name, "Chat");
        assert!(b.project_role_assertion);
        assert!(!b.project_role_check);
        assert!(b.has_project_check);
    }
```

- [ ] **Step 2: Run it, expect FAIL.** `cargo test -p llm-chat-admin-api --lib contract_tests::update_project_accepts_camelcase` — expect `cannot find type 'UpdateProject' in this scope`.

- [ ] **Step 3: Implement.** Add the four routes-pairs to `router()` in `admin-api/src/api/mod.rs` — insert after the `.route("/api/roles", ...)` line:

```rust
        .route("/api/project", get(get_project).put(update_project))
        .route("/api/org/policies/login", get(get_login_policy).put(put_login_policy))
        .route("/api/org/policies/password-complexity",
            get(get_password_complexity).put(put_password_complexity))
        .route("/api/org/policies/lockout", get(get_lockout).put(put_lockout))
```

Then add the handlers + the request struct before the `#[cfg(test)] mod contract_tests` block:

```rust
async fn get_project(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_project().await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProject {
    name: String,
    #[serde(default)] project_role_assertion: bool,
    #[serde(default)] project_role_check: bool,
    #[serde(default)] has_project_check: bool,
}
async fn update_project(_op: Operator, State(st): State<AppState>, Json(b): Json<UpdateProject>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.update_project(
        &b.name, b.project_role_assertion, b.project_role_check, b.has_project_check,
    ).await?))
}

// Policies: the body is the operator's raw camelCase JSON (Duration strings and
// all), passed straight to the upsert. No reshaping — Zitadel owns the schema.
async fn get_login_policy(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_login_policy().await?))
}
async fn put_login_policy(_op: Operator, State(st): State<AppState>, Json(b): Json<Value>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.upsert_login_policy(&b).await?))
}
async fn get_password_complexity(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_password_complexity_policy().await?))
}
async fn put_password_complexity(_op: Operator, State(st): State<AppState>, Json(b): Json<Value>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.upsert_password_complexity_policy(&b).await?))
}
async fn get_lockout(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_lockout_policy().await?))
}
async fn put_lockout(_op: Operator, State(st): State<AppState>, Json(b): Json<Value>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.upsert_lockout_policy(&b).await?))
}
```

- [ ] **Step 4: Run it, expect PASS.** `cargo test -p llm-chat-admin-api --lib contract_tests` — expect all contract tests pass including `update_project_accepts_camelcase`.

- [ ] **Step 5: Commit.**
```
git add admin-api/src/api/mod.rs
git commit -m "feat(admin-api): /api/project + /api/org/policies/{login,password-complexity,lockout} routes (design §9)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.4: ADMIN_IT test — nail the real upsert error code on a default policy

- [ ] **Step 1: Write the failing test.** Append a new `#[tokio::test]` to `admin-api/tests/integration.rs` (after `create_grant_key_lifecycle_full_coverage`). It drives the **real** upsert and asserts the round-trip; the critical assertion is that a PUT against the unverified policy returns the typed branch our `upsert_policy` keys on:

```rust
#[tokio::test]
async fn it_policy_upsert_round_trips_and_pins_not_found_branch() {
    if !it_enabled() {
        eprintln!("ADMIN_IT!=1 — skipping policy upsert IT (design §9 upsert trap)");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let z = admin_client(cfg, http);

    // GET must succeed even when the org is on the instance DEFAULT policy
    // (Zitadel serves the default through the same endpoint, isDefault==true).
    let login = z.get_login_policy().await.expect("get_login_policy");
    let mfa_skip = login.get("mfaInitSkipLifetime").cloned();
    eprintln!("login.isDefault={:?} mfaInitSkipLifetime={:?}",
        login.get("isDefault"), mfa_skip);

    // UPSERT: send the policy back with mfaInitSkipLifetime as a Duration STRING.
    // This exercises PUT; on a default-only org the PUT 404s and upsert_policy
    // POSTs the add-custom variant. Either way the result must be Ok — that is
    // the whole point of pinning the NotFound branch (design §9).
    let body = serde_json::json!({
        "allowUsernamePassword": true,
        "allowRegister": true,
        "allowExternalIdp": false,
        "forceMfa": false,
        "passwordlessType": "PASSWORDLESS_TYPE_NOT_ALLOWED",
        "mfaInitSkipLifetime": "0s"
    });
    let updated = z.upsert_login_policy(&body).await.expect("upsert_login_policy");
    eprintln!("upsert ok: {updated}");

    // Read back: the custom policy now exists and the Duration string survived.
    let after = z.get_login_policy().await.expect("get_login_policy after");
    assert_eq!(
        after.get("mfaInitSkipLifetime").and_then(|v| v.as_str()),
        Some("0s"),
        "mfaInitSkipLifetime must round-trip as the Duration string we sent"
    );

    // Lockout + password-complexity GET must also succeed (smoke the other two).
    z.get_lockout_policy().await.expect("get_lockout_policy");
    z.get_password_complexity_policy().await.expect("get_password_complexity_policy");
}
```

- [ ] **Step 2: Run it, expect FAIL.** Without the running instance it is skipped (prints the skip line) — to actually exercise it against the stack:
`$env:ADMIN_IT="1"; cargo test -p llm-chat-admin-api --test integration it_policy_upsert_round_trips_and_pins_not_found_branch -- --nocapture`
Before Tasks 3.1–3.2 are merged this fails to **compile** (`no method named 'get_login_policy'`); with them merged but the SA still on `ORG_USER_MANAGER` it fails with `Forbidden` on the PUT — which is the live proof the **ORG_OWNER bump (Phase 0 §5)** is a hard prerequisite. Record whichever error you observe (`NotFound` add-custom path vs `Ok` direct-PUT path) so §14 risk 3 is discharged.

- [ ] **Step 3: Implement.** No new product code — Tasks 3.1–3.3 already provide every method. If the live run shows the PUT returns **400** (FAILED_PRECONDITION) instead of 404 on a default org, that contradicts our `should_add_custom` branch; fix it at the source by widening the branch in `policies.rs` to also fall back on the exact `Invalid` message Zitadel returns (do NOT broaden blindly — match the precise text), and update the `upsert_falls_back_only_on_not_found` unit test to encode the verified shape. Otherwise leave the code unchanged.

- [ ] **Step 4: Run it, expect PASS (with the stack).** `$env:ADMIN_IT="1"; cargo test -p llm-chat-admin-api --test integration it_policy_upsert_round_trips_and_pins_not_found_branch -- --nocapture` — expect `test result: ok. 1 passed` and the `upsert ok:` / `isDefault` lines in the captured output. With `ADMIN_IT` unset: `cargo test -p llm-chat-admin-api --test integration` still compiles and the test self-skips.

- [ ] **Step 5: Commit.**
```
git add admin-api/tests/integration.rs
git commit -m "test(admin-api): ADMIN_IT pins the policy-upsert NotFound branch + Duration round-trip (design §9, §14.3)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.5: Frontend types for the four cards

- [ ] **Step 1: Write the failing test.** Create `admin-web/__tests__/policy-cards.test.tsx` with a type-level + render guard that imports the new types and the (not-yet-existing) card; this fails to resolve the import:

```tsx
import { describe, it, expect } from "vitest";
import type { LoginPolicy } from "../lib/types";

describe("policy types", () => {
  it("LoginPolicy.mfaInitSkipLifetime is a Duration STRING, not a number", () => {
    // protobuf Duration serializes as a string ("0s", "720h0m0s") — design §9.
    const p: LoginPolicy = {
      allowUsernamePassword: true,
      allowRegister: true,
      allowExternalIdp: false,
      forceMfa: false,
      passwordlessType: "PASSWORDLESS_TYPE_NOT_ALLOWED",
      mfaInitSkipLifetime: "0s",
    };
    expect(typeof p.mfaInitSkipLifetime).toBe("string");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL.** `cd admin-web; pnpm test policy-cards` — expect `Module '"../lib/types"' has no exported member 'LoginPolicy'` (type error / failing run).

- [ ] **Step 3: Implement.** Append to `admin-web/lib/types.ts`:

```typescript
export interface Project {
  id: string;
  name: string;
  projectRoleAssertion?: boolean;
  projectRoleCheck?: boolean;
  hasProjectCheck?: boolean;
}

// protobuf Duration fields arrive as STRINGS ("0s", "720h0m0s") — design §9.
export interface LoginPolicy {
  allowUsernamePassword: boolean;
  allowRegister: boolean;
  allowExternalIdp: boolean;
  forceMfa: boolean;
  passwordlessType: string;
  mfaInitSkipLifetime: string;
  isDefault?: boolean;
}

export interface PasswordComplexityPolicy {
  minLength: string; // uint64 serializes as a string on this build
  hasUppercase: boolean;
  hasLowercase: boolean;
  hasNumber: boolean;
  hasSymbol: boolean;
  isDefault?: boolean;
}

export interface LockoutPolicy {
  maxPasswordAttempts: string; // uint64 -> string
  isDefault?: boolean;
}
```

- [ ] **Step 4: Run it, expect PASS.** `cd admin-web; pnpm test policy-cards` — expect `1 passed`.

- [ ] **Step 5: Commit.**
```
git add admin-web/lib/types.ts admin-web/__tests__/policy-cards.test.tsx
git commit -m "feat(admin-web): Project + policy types (Duration/uint64 as strings, design §9)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.6: `components/project/policy-cards.tsx` — the four edit-form cards

- [ ] **Step 1: Write the failing test.** Extend `admin-web/__tests__/policy-cards.test.tsx` to render the login card with a Duration value and assert the input shows the string verbatim (this fails — the component doesn't exist):

```tsx
import { render, screen, cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
import { LoginPolicyCard } from "../components/project/policy-cards";

afterEach(cleanup);

describe("LoginPolicyCard", () => {
  it("shows mfaInitSkipLifetime Duration string verbatim in its input", () => {
    render(
      <LoginPolicyCard
        value={{
          allowUsernamePassword: true,
          allowRegister: true,
          allowExternalIdp: false,
          forceMfa: false,
          passwordlessType: "PASSWORDLESS_TYPE_NOT_ALLOWED",
          mfaInitSkipLifetime: "720h0m0s",
        }}
        onSaved={() => {}}
      />,
    );
    expect(screen.getByLabelText(/2FA-setup skip lifetime/i)).toHaveValue("720h0m0s");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL.** `cd admin-web; pnpm test policy-cards` — expect `Failed to resolve import "../components/project/policy-cards"`.

- [ ] **Step 3: Implement.** Create `admin-web/components/project/policy-cards.tsx`. Each card mirrors `create-user-dialog.tsx` (react-hook-form + `zodResolver`, `api.put` + `toast` + `ApiError`) but renders inside a `Card` instead of a `Dialog`, using the Phase-0 `switch` primitive:

```tsx
"use client";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle,
} from "@/components/ui/card";
import {
  Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { api, ApiError } from "@/lib/api";
import type {
  Project, LoginPolicy, PasswordComplexityPolicy, LockoutPolicy,
} from "@/lib/types";

// ---- Project ----
const projectSchema = z.object({
  name: z.string().min(1),
  projectRoleAssertion: z.boolean(),
  projectRoleCheck: z.boolean(),
  hasProjectCheck: z.boolean(),
});
type ProjectValues = z.infer<typeof projectSchema>;

export function ProjectCard({ value, onSaved }: { value: Project; onSaved: () => void }) {
  const form = useForm<ProjectValues>({
    resolver: zodResolver(projectSchema),
    defaultValues: {
      name: value.name,
      projectRoleAssertion: value.projectRoleAssertion ?? false,
      projectRoleCheck: value.projectRoleCheck ?? false,
      hasProjectCheck: value.hasProjectCheck ?? false,
    },
  });
  useEffect(() => {
    form.reset({
      name: value.name,
      projectRoleAssertion: value.projectRoleAssertion ?? false,
      projectRoleCheck: value.projectRoleCheck ?? false,
      hasProjectCheck: value.hasProjectCheck ?? false,
    });
  }, [value, form]);

  async function onSubmit(v: ProjectValues) {
    try {
      await api.put("/api/project", v);
      toast.success("Project updated");
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Project</CardTitle>
        <CardDescription>The Chat app's name and role-assertion flags.</CardDescription>
      </CardHeader>
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)}>
          <CardContent className="space-y-4">
            <FormField control={form.control} name="name" render={({ field }) => (
              <FormItem><FormLabel>Name</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="projectRoleAssertion" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Assert roles in tokens</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="projectRoleCheck" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Check roles on authentication</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="hasProjectCheck" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Require project authorization</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
          </CardContent>
          <CardFooter><Button type="submit">Save project</Button></CardFooter>
        </form>
      </Form>
    </Card>
  );
}

// ---- Login policy ----
const loginSchema = z.object({
  allowUsernamePassword: z.boolean(),
  allowRegister: z.boolean(),
  allowExternalIdp: z.boolean(),
  forceMfa: z.boolean(),
  passwordlessType: z.string(),
  mfaInitSkipLifetime: z.string(), // Duration string, e.g. "0s" — design §9
});
type LoginValues = z.infer<typeof loginSchema>;

export function LoginPolicyCard({ value, onSaved }: { value: LoginPolicy; onSaved: () => void }) {
  const form = useForm<LoginValues>({
    resolver: zodResolver(loginSchema),
    defaultValues: {
      allowUsernamePassword: value.allowUsernamePassword,
      allowRegister: value.allowRegister,
      allowExternalIdp: value.allowExternalIdp,
      forceMfa: value.forceMfa,
      passwordlessType: value.passwordlessType,
      mfaInitSkipLifetime: value.mfaInitSkipLifetime,
    },
  });
  useEffect(() => {
    form.reset({
      allowUsernamePassword: value.allowUsernamePassword,
      allowRegister: value.allowRegister,
      allowExternalIdp: value.allowExternalIdp,
      forceMfa: value.forceMfa,
      passwordlessType: value.passwordlessType,
      mfaInitSkipLifetime: value.mfaInitSkipLifetime,
    });
  }, [value, form]);

  async function onSubmit(v: LoginValues) {
    try {
      // Send the Duration string through untouched — never reparse it (design §9).
      await api.put("/api/org/policies/login", v);
      toast.success("Login policy updated");
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Login policy</CardTitle>
        <CardDescription>
          Sign-in options + the 2FA-setup nudge. Set the skip lifetime to <code>0s</code> to
          stop prompting new users to set up 2FA.
        </CardDescription>
      </CardHeader>
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)}>
          <CardContent className="space-y-4">
            <FormField control={form.control} name="allowUsernamePassword" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Allow username + password</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="allowRegister" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Allow self-registration</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="allowExternalIdp" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Allow external IdP</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="forceMfa" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Force MFA</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="passwordlessType" render={({ field }) => (
              <FormItem><FormLabel>Passwordless</FormLabel>
                <Select onValueChange={field.onChange} value={field.value}>
                  <FormControl><SelectTrigger><SelectValue /></SelectTrigger></FormControl>
                  <SelectContent>
                    <SelectItem value="PASSWORDLESS_TYPE_NOT_ALLOWED">Not allowed</SelectItem>
                    <SelectItem value="PASSWORDLESS_TYPE_ALLOWED">Allowed</SelectItem>
                  </SelectContent>
                </Select>
                <FormMessage />
              </FormItem>
            )} />
            <FormField control={form.control} name="mfaInitSkipLifetime" render={({ field }) => (
              <FormItem><FormLabel>2FA-setup skip lifetime</FormLabel>
                <FormControl><Input {...field} placeholder="0s" /></FormControl>
                <FormDescription>
                  Protobuf duration string (e.g. <code>0s</code>, <code>720h0m0s</code>). <code>0s</code> disables the nudge.
                </FormDescription>
                <FormMessage /></FormItem>
            )} />
          </CardContent>
          <CardFooter><Button type="submit">Save login policy</Button></CardFooter>
        </form>
      </Form>
    </Card>
  );
}

// ---- Password complexity ----
const pwSchema = z.object({
  minLength: z.string(),
  hasUppercase: z.boolean(),
  hasLowercase: z.boolean(),
  hasNumber: z.boolean(),
  hasSymbol: z.boolean(),
});
type PwValues = z.infer<typeof pwSchema>;

export function PasswordComplexityCard(
  { value, onSaved }: { value: PasswordComplexityPolicy; onSaved: () => void },
) {
  const form = useForm<PwValues>({
    resolver: zodResolver(pwSchema),
    defaultValues: {
      minLength: value.minLength,
      hasUppercase: value.hasUppercase,
      hasLowercase: value.hasLowercase,
      hasNumber: value.hasNumber,
      hasSymbol: value.hasSymbol,
    },
  });
  useEffect(() => {
    form.reset({
      minLength: value.minLength,
      hasUppercase: value.hasUppercase,
      hasLowercase: value.hasLowercase,
      hasNumber: value.hasNumber,
      hasSymbol: value.hasSymbol,
    });
  }, [value, form]);

  async function onSubmit(v: PwValues) {
    try {
      await api.put("/api/org/policies/password-complexity", v);
      toast.success("Password policy updated");
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Password complexity</CardTitle>
        <CardDescription>Minimum length + required character classes.</CardDescription>
      </CardHeader>
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)}>
          <CardContent className="space-y-4">
            <FormField control={form.control} name="minLength" render={({ field }) => (
              <FormItem><FormLabel>Minimum length</FormLabel>
                <FormControl><Input inputMode="numeric" {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="hasUppercase" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Require uppercase</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="hasLowercase" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Require lowercase</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="hasNumber" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Require number</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="hasSymbol" render={({ field }) => (
              <FormItem className="flex items-center justify-between">
                <FormLabel>Require symbol</FormLabel>
                <FormControl><Switch checked={field.value} onCheckedChange={field.onChange} /></FormControl>
              </FormItem>
            )} />
          </CardContent>
          <CardFooter><Button type="submit">Save password policy</Button></CardFooter>
        </form>
      </Form>
    </Card>
  );
}

// ---- Lockout ----
const lockoutSchema = z.object({ maxPasswordAttempts: z.string() });
type LockoutValues = z.infer<typeof lockoutSchema>;

export function LockoutCard({ value, onSaved }: { value: LockoutPolicy; onSaved: () => void }) {
  const form = useForm<LockoutValues>({
    resolver: zodResolver(lockoutSchema),
    defaultValues: { maxPasswordAttempts: value.maxPasswordAttempts },
  });
  useEffect(() => {
    form.reset({ maxPasswordAttempts: value.maxPasswordAttempts });
  }, [value, form]);

  async function onSubmit(v: LockoutValues) {
    try {
      await api.put("/api/org/policies/lockout", v);
      toast.success("Lockout policy updated");
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Lockout</CardTitle>
        <CardDescription>Failed-password attempts before an account locks (0 = never).</CardDescription>
      </CardHeader>
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)}>
          <CardContent>
            <FormField control={form.control} name="maxPasswordAttempts" render={({ field }) => (
              <FormItem><FormLabel>Max password attempts</FormLabel>
                <FormControl><Input inputMode="numeric" {...field} /></FormControl><FormMessage /></FormItem>
            )} />
          </CardContent>
          <CardFooter><Button type="submit">Save lockout policy</Button></CardFooter>
        </form>
      </Form>
    </Card>
  );
}
```

- [ ] **Step 4: Run it, expect PASS.** `cd admin-web; pnpm test policy-cards` — expect `3 passed` (the two type tests + the `LoginPolicyCard` render).

- [ ] **Step 5: Commit.**
```
git add admin-web/components/project/policy-cards.tsx admin-web/__tests__/policy-cards.test.tsx
git commit -m "feat(admin-web): Project + policy edit cards (switch/input/select, Duration string preserved, design §9)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3.7: `app/(dash)/project/page.tsx` — the page + NAV entry

- [ ] **Step 1: Write the failing test.** Add to `admin-web/e2e/smoke.spec.ts`, inside the `test.describe("authenticated operator flow", ...)` block (after the existing create-user test), the policy-edit spec that the page must satisfy:

```typescript
  test("edit login policy: toggle a switch and save", async ({ page }) => {
    await page.goto("/login");
    await page.locator('input[name="loginName"]').fill(process.env.ADMIN_IT_USER!);
    await page.getByRole("button", { name: /next|continue/i }).click();
    await page.locator('input[name="password"]').fill(process.env.ADMIN_IT_PASS!);
    await page.getByRole("button", { name: /next|continue|sign in/i }).click();
    const skip2fa = page.getByRole("button", { name: /skip/i });
    await skip2fa.waitFor({ state: "visible", timeout: 8000 })
      .then(() => skip2fa.click()).catch(() => {});

    // Navigate to Project via the shell nav (design §2 single-source NAV).
    await page.getByRole("link", { name: "Project" }).click();
    await page.waitForURL(/\/project/);
    await expect(page.getByRole("heading", { name: "Login policy" })).toBeVisible();

    // Set the 2FA-setup skip lifetime to 0s (the demo-login fix) and save.
    const skipInput = page.getByLabel(/2FA-setup skip lifetime/i);
    await skipInput.fill("0s");
    await page.getByRole("button", { name: "Save login policy" }).click();
    await expect(page.getByText(/login policy updated/i)).toBeVisible();
  });
```

- [ ] **Step 2: Run it, expect FAIL.** `cd admin-web; $env:ADMIN_IT="1"; pnpm e2e -g "edit login policy"` — expect failure: the `Project` nav link / `/project` route does not exist yet (timeout on `getByRole("link", { name: "Project" })`).

- [ ] **Step 3: Implement.** Create `admin-web/app/(dash)/project/page.tsx` mirroring `users/page.tsx` (the `useCallback load()` → `useState`, `useEffect` on mount, reload after each save; the page chrome — `<h1>`, sign-out — is owned by the Phase-0 shell, so this page renders bare cards):

```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import {
  ProjectCard, LoginPolicyCard, PasswordComplexityCard, LockoutCard,
} from "@/components/project/policy-cards";
import { api, ApiError } from "@/lib/api";
import type {
  Project, LoginPolicy, PasswordComplexityPolicy, LockoutPolicy,
} from "@/lib/types";

export default function ProjectPage() {
  const [project, setProject] = useState<Project | null>(null);
  const [login, setLogin] = useState<LoginPolicy | null>(null);
  const [password, setPassword] = useState<PasswordComplexityPolicy | null>(null);
  const [lockout, setLockout] = useState<LockoutPolicy | null>(null);

  const load = useCallback(async () => {
    // Each card degrades independently: one failed GET must not blank the page
    // (design §12). 401 is swallowed — lib/api already redirects to /login.
    const settle = async <T,>(p: Promise<T>, set: (v: T) => void, what: string) => {
      try {
        set(await p);
      } catch (e) {
        if (!(e instanceof ApiError && e.status === 401)) {
          toast.error(e instanceof ApiError ? e.message : `Failed to load ${what}`);
        }
      }
    };
    await Promise.all([
      settle(api.get<Project>("/api/project"), setProject, "project"),
      settle(api.get<LoginPolicy>("/api/org/policies/login"), setLogin, "login policy"),
      settle(api.get<PasswordComplexityPolicy>("/api/org/policies/password-complexity"),
        setPassword, "password policy"),
      settle(api.get<LockoutPolicy>("/api/org/policies/lockout"), setLockout, "lockout policy"),
    ]);
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <main className="container mx-auto py-8 space-y-6">
      <h1 className="text-2xl font-semibold">Project & org settings</h1>
      <div className="grid gap-6 md:grid-cols-2">
        {project && <ProjectCard value={project} onSaved={load} />}
        {login && <LoginPolicyCard value={login} onSaved={load} />}
        {password && <PasswordComplexityCard value={password} onSaved={load} />}
        {lockout && <LockoutCard value={lockout} onSaved={load} />}
      </div>
    </main>
  );
}
```

Then append the NAV entry to the Phase-0 array in `admin-web/components/shell/nav.ts` (insert before the `Audit`/`ScrollText` entry, keeping the §1 build order). Add `Building2` to the lucide import if not already present:

```typescript
  { icon: Building2, label: "Project", href: "/project", match: "/project" },
```

- [ ] **Step 4: Run it, expect PASS.** `cd admin-web; $env:ADMIN_IT="1"; pnpm e2e -g "edit login policy"` — expect `1 passed`. Also confirm the offline build is clean: `cd admin-web; pnpm build` succeeds (Next.js 16 route-group `(dash)` adds no URL segment, so `/project` resolves).

- [ ] **Step 5: Commit.**
```
git add admin-web/app/(dash)/project/page.tsx admin-web/components/shell/nav.ts admin-web/e2e/smoke.spec.ts
git commit -m "feat(admin-web): Project & org settings page + nav + policy-edit e2e (design §9)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

**Phase 3 done-criteria:** `cargo test -p llm-chat-admin-api` green offline; `cd admin-web; pnpm test && pnpm build` green; with the stack up, `$env:ADMIN_IT="1"; cargo test -p llm-chat-admin-api --test integration it_policy_upsert_round_trips_and_pins_not_found_branch -- --nocapture` and `pnpm e2e -g "edit login policy"` both pass. The Project ribbon entry is navigable and an operator can set `mfaInitSkipLifetime` to `0s` to stop the 2FA-setup nudge. **Risk §14.3 (policy-upsert error code) is discharged** by recording the observed branch in the ADMIN_IT run; **§14.1/§5 ORG_OWNER dependency is proven** if the live PUT returns `Forbidden` before the bump.

## Phase 4: Dashboard (spec §10)

Aggregates the platform's existing `_search` surfaces into a single `GET /api/stats` fan-out (users by type, roles, grants, apps) plus a `valid_token()` health self-check, and renders them as colourful, deep-linking stat cards that become the Console's default landing screen.

**Depends on:** Phase 0 (the `(dash)/layout.tsx` shell, the `NAV` array with the `LayoutDashboard` Dashboard entry already wired, and the shadcn `card` primitive at `components/ui/card.tsx`), Phase 1 (`list_roles`, `list_user_grants`), and Phase 2 (`zitadel/apps.rs` with `search_apps`). Every Zitadel path below is grounded in the spec's verified `_search` tables (§7, §8, §10) and `provision.py` — no new Zitadel surface beyond apps `_search`, which Phase 2 already added.

**The string-or-number trap (§10, §14.6):** `details.totalResult` serialises as a JSON **string** in some Zitadel builds and a **number** in others; a naive `as_u64()` silently reads `0`. The count helper MUST try `as_u64()` **and** `as_str().parse()`. Each count is independent: one failing fan-out call degrades that card to `null` (em-dash), never blanking the page (§12).

**Files:**

- `admin-api/src/zitadel/stats.rs` (Create) — pure `count_from_total` helper (parses `details.totalResult` as u64-or-string) + `ZitadelClient` count fan-out methods (`count_humans`, `count_machines`, `count_roles`, `count_grants`, `count_apps`), each returning `Option<u64>` (None on its own failure).
- `admin-api/src/zitadel/mod.rs` (Modify) — add `pub mod stats;`.
- `admin-api/src/api/mod.rs` (Modify) — add the `GET /api/stats` route + `stats` handler returning camelCase counts + `tokenHealthy` self-check.
- `admin-web/lib/types.ts` (Modify) — add the `Stats` interface mirroring the BFF JSON.
- `admin-web/app/(dash)/dashboard/page.tsx` (Create) — colourful stat cards (mockup colours) that deep-link into each section.
- `admin-web/app/(dash)/page.tsx` (Create) — the `(dash)` index; `redirect("/dashboard")` so the Console lands on the Dashboard.
- `admin-web/__tests__/dashboard.test.tsx` (Create) — renders the cards from a stubbed `api.get`, asserts numbers + deep-links (and em-dash on `null`).
- `admin-web/e2e/smoke.spec.ts` (Modify) — add an `ADMIN_IT`-gated Dashboard cards-render check.

---

### Task 4.1: `count_from_total` parses `details.totalResult` as both number and string

Pure helper isolating the §10/§14.6 trap. It reads `details.totalResult` from a `_search` response `Value` and returns `Option<u64>`, trying `as_u64()` first, then `as_str().parse::<u64>()`.

- [ ] **Step 1: Write the failing test.** Create `admin-api/src/zitadel/stats.rs` with ONLY the test module (no impl yet) so the test names the function that does not exist:

```rust
//! Dashboard fan-out: per-area `totalResult` counts from existing `_search`
//! endpoints (design §10) + the string-or-number parse trap (§14.6). Each count
//! is independent: a single failing call degrades that card to `null`, it never
//! blanks the page (§12).

#[cfg(test)]
mod tests {
    use super::count_from_total;
    use serde_json::json;

    #[test]
    fn count_from_total_reads_number_form() {
        // Some Zitadel builds serialize totalResult as a JSON number.
        let v = json!({ "details": { "totalResult": 42 }, "result": [] });
        assert_eq!(count_from_total(&v), Some(42));
    }

    #[test]
    fn count_from_total_reads_string_form() {
        // Other builds serialize the SAME field as a JSON string (§14.6).
        let v = json!({ "details": { "totalResult": "42" }, "result": [] });
        assert_eq!(count_from_total(&v), Some(42));
    }

    #[test]
    fn count_from_total_missing_is_none() {
        // No details/totalResult (or a non-numeric string) -> None, not 0, so the
        // card shows an em-dash rather than a misleading zero.
        assert_eq!(count_from_total(&json!({ "result": [] })), None);
        assert_eq!(count_from_total(&json!({ "details": { "totalResult": "abc" } })), None);
    }
}
```

- [ ] **Step 2: Run it, expect FAIL.** `cargo test -p llm-chat-admin-api --lib zitadel::stats` — fails to compile: `cannot find function 'count_from_total' in this scope` (the `use super::count_from_total;` resolves to nothing).

- [ ] **Step 3: Implement.** Add the helper above the test module in `admin-api/src/zitadel/stats.rs`:

```rust
use serde_json::Value;

use super::ZitadelClient;

/// PURE: extract `details.totalResult` as a count. Zitadel serializes this field
/// as a JSON **number** in some builds and a JSON **string** in others (§14.6);
/// try `as_u64` first, then `as_str().parse`. Returns `None` when the field is
/// absent or unparseable so the card degrades to an em-dash, never a false `0`.
pub fn count_from_total(v: &Value) -> Option<u64> {
    let total = v.get("details")?.get("totalResult")?;
    total
        .as_u64()
        .or_else(|| total.as_str().and_then(|s| s.parse::<u64>().ok()))
}
```

- [ ] **Step 4: Run it, expect PASS.** `cargo test -p llm-chat-admin-api --lib zitadel::stats` — `test result: ok. 3 passed`.

- [ ] **Step 5: Commit.**

```
git add admin-api/src/zitadel/stats.rs
git commit -m "feat(admin-api): count_from_total — parse totalResult as number or string

Zitadel serializes details.totalResult as a JSON string in some builds; a
naive as_u64 reads 0. Try as_u64 then as_str().parse so dashboard counts are
real (design §10/§14.6); None on absent/unparseable -> card shows em-dash.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4.2: `count_*` fan-out methods — each `_search` count, degrading to `None`

Five `ZitadelClient` methods that POST the area's `_search` endpoint with a count-only filter and run the result through `count_from_total`. Each maps any `ZitadelError` to `None` (`.ok().and_then(...)`) so one failing call degrades only its own card. Endpoints are exactly those the spec verifies: `/v2/users` (type-filtered), `projects/{pid}/roles/_search`, `users/grants/_search`, `projects/{pid}/apps/_search`.

- [ ] **Step 1: Write the failing test.** Append a second test module to `admin-api/src/zitadel/stats.rs` that asserts the method signatures exist and return `Option<u64>` (a compile-level contract; the live values are covered by the `ADMIN_IT` integration test, not here):

```rust
#[cfg(test)]
mod method_contract {
    use super::ZitadelClient;

    // Compile-time contract: the fan-out methods exist with the Option<u64>
    // shape the /api/stats handler relies on. Their live values are exercised by
    // tests/integration.rs under ADMIN_IT=1 (the real instance is the source of
    // truth, not a mocked _search body).
    #[allow(dead_code)]
    fn signatures_compile(z: &ZitadelClient) {
        let _: fn(&ZitadelClient) -> _ = |z| z.count_humans();
        let _: fn(&ZitadelClient) -> _ = |z| z.count_machines();
        let _: fn(&ZitadelClient) -> _ = |z| z.count_roles();
        let _: fn(&ZitadelClient) -> _ = |z| z.count_grants();
        let _: fn(&ZitadelClient) -> _ = |z| z.count_apps();
        let _ = z;
    }
}
```

- [ ] **Step 2: Run it, expect FAIL.** `cargo test -p llm-chat-admin-api --lib zitadel::stats` — fails to compile: `no method named 'count_humans' found for reference '&ZitadelClient'`.

- [ ] **Step 3: Implement.** Add the `impl` block between the `count_from_total` helper and the test modules in `admin-api/src/zitadel/stats.rs`:

```rust
impl ZitadelClient {
    /// Count human users via v2 (`POST /v2/users`, type-filtered like §3.1).
    /// `None` on its own failure so the card degrades to an em-dash (§12).
    pub async fn count_humans(&self) -> Option<u64> {
        let url = format!("{}/v2/users", self.cfg.issuer);
        let body = json!({ "queries": [{ "typeQuery": { "type": "TYPE_HUMAN" } }] });
        self.post_json(&url, &body).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count machine users via v2 (`POST /v2/users`, type-filtered).
    pub async fn count_machines(&self) -> Option<u64> {
        let url = format!("{}/v2/users", self.cfg.issuer);
        let body = json!({ "queries": [{ "typeQuery": { "type": "TYPE_MACHINE" } }] });
        self.post_json(&url, &body).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count project roles (`POST .../projects/{pid}/roles/_search`, §7).
    pub async fn count_roles(&self) -> Option<u64> {
        let url = format!("{}/management/v1/projects/{}/roles/_search", self.cfg.issuer, self.cfg.project_id);
        self.post_json(&url, &json!({})).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count user grants (`POST /management/v1/users/grants/_search`, §7).
    pub async fn count_grants(&self) -> Option<u64> {
        let url = format!("{}/management/v1/users/grants/_search", self.cfg.issuer);
        self.post_json(&url, &json!({})).await.ok().and_then(|v| count_from_total(&v))
    }

    /// Count this project's apps (`POST .../projects/{pid}/apps/_search`, §8).
    pub async fn count_apps(&self) -> Option<u64> {
        let url = format!("{}/management/v1/projects/{}/apps/_search", self.cfg.issuer, self.cfg.project_id);
        self.post_json(&url, &json!({})).await.ok().and_then(|v| count_from_total(&v))
    }
}
```

Update the top `use` line of the file so `json!` is in scope:

```rust
use serde_json::{json, Value};
```

- [ ] **Step 4: Run it, expect PASS.** `cargo test -p llm-chat-admin-api --lib zitadel::stats` — `test result: ok. 3 passed` (the method-contract module is a compile-only check; it contributes no runtime assertions but the crate now compiles).

- [ ] **Step 5: Commit.**

```
git add admin-api/src/zitadel/stats.rs
git commit -m "feat(admin-api): count fan-out methods over existing _search endpoints

count_humans/machines/roles/grants/apps each POST the area's verified _search
(design §7/§8/§10) and run the body through count_from_total; each maps its own
error to None so one failing fan-out call degrades only its card (§12).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4.3: Wire `stats` into the Zitadel module

- [ ] **Step 1: Write the failing test.** No new test file — the failing signal is the crate not compiling because `stats.rs` is an orphan module. Add a one-line doc assertion to the existing `stats` test by referencing the module path from the crate root; instead, drive it via the build: run the existing suite which will not see `stats` until it is declared.

  Concretely, confirm the orphan state first: `cargo test -p llm-chat-admin-api --lib zitadel::stats` currently errors with `file not found for module` resolution only once referenced — so make the reference. Edit `admin-api/src/zitadel/mod.rs`, locate the module declarations:

```rust
pub mod error;
pub mod grants;
pub mod keys;
pub mod model;
pub mod token;
pub mod users;
```

- [ ] **Step 2: Run it, expect FAIL.** Before editing, `cargo build -p llm-chat-admin-api` succeeds but **does not compile `stats.rs`** (it is not a declared module, so its tests never run): `cargo test -p llm-chat-admin-api --lib zitadel::stats::tests::count_from_total_reads_string_form` prints `0 tests run` / `no tests to run`. That "0 tests" is the failing state — the module is dead.

- [ ] **Step 3: Implement.** Add `stats` to the declarations (alphabetical-after-roles placement, mirroring the existing list ordering by keeping `users` last as it is in the file — insert `stats` before `token`):

```rust
pub mod error;
pub mod grants;
pub mod keys;
pub mod model;
pub mod stats;
pub mod token;
pub mod users;
```

- [ ] **Step 4: Run it, expect PASS.** `cargo test -p llm-chat-admin-api --lib zitadel::stats` — now `test result: ok. 3 passed` (the module is live and its tests execute).

- [ ] **Step 5: Commit.**

```
git add admin-api/src/zitadel/mod.rs
git commit -m "feat(admin-api): declare zitadel::stats module

Wire the dashboard fan-out module into the only crate that touches Zitadel so
its count_* methods + count_from_total tests are live.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4.4: `GET /api/stats` handler — camelCase counts + token-health self-check

A thin `Operator`-gated handler that fans the five counts out concurrently and adds a `valid_token()` boolean health probe, returning the camelCase JSON the frontend consumes. Mirrors the `me`/`list_roles` handler shape in `api/mod.rs`.

- [ ] **Step 1: Write the failing test.** Append a camelCase **contract** test to the existing `contract_tests` module at the bottom of `admin-api/src/api/mod.rs`, asserting the response shape the handler must emit. Since the handler needs live Zitadel, the unit test pins the JSON keys via the `StatsResponse` struct (added in Step 3) rather than calling the handler:

```rust
    #[test]
    fn stats_response_serializes_camelcase() {
        let s = StatsResponse {
            humans: Some(18),
            machines: Some(6),
            roles: Some(3),
            grants: Some(40),
            apps: Some(3),
            token_healthy: true,
        };
        let v = serde_json::to_value(&s).expect("serialize StatsResponse");
        assert_eq!(v.get("humans").and_then(|x| x.as_u64()), Some(18));
        assert!(v.get("tokenHealthy").and_then(|x| x.as_bool()).unwrap(), "camelCase tokenHealthy: {v}");
        assert!(v.get("token_healthy").is_none(), "no snake_case: {v}");
        // A failed count must serialize as JSON null (em-dash on the card), not 0.
        let degraded = serde_json::to_value(StatsResponse {
            humans: None, machines: None, roles: None, grants: None, apps: None,
            token_healthy: false,
        }).unwrap();
        assert!(degraded.get("humans").unwrap().is_null(), "null count: {degraded}");
    }
```

- [ ] **Step 2: Run it, expect FAIL.** `cargo test -p llm-chat-admin-api --lib api::contract_tests::stats_response_serializes_camelcase` — fails to compile: `cannot find type 'StatsResponse' in this scope`.

- [ ] **Step 3: Implement.** In `admin-api/src/api/mod.rs`, add the `Serialize` import, the response struct, and the handler. First extend the `serde` import at the top of the file:

```rust
use serde::{Deserialize, Serialize};
```

  Add the route inside `router(...)` next to the other gated read routes (right after the `/api/roles` line):

```rust
        .route("/api/roles", get(list_roles))
        .route("/api/stats", get(stats))
```

  Add the struct + handler (place after `list_roles`, before `list_grants`):

```rust
/// Dashboard counts (design §10). Each count is `Option<u64>` — `null` in JSON
/// when its own fan-out call failed, so the card shows an em-dash, never a false
/// `0` (§12). camelCase preserved for the frontend `Stats` type.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsResponse {
    humans: Option<u64>,
    machines: Option<u64>,
    roles: Option<u64>,
    grants: Option<u64>,
    apps: Option<u64>,
    token_healthy: bool,
}

/// GET /api/stats — fan out the per-area `totalResult` counts + a SA-token
/// health self-check. Counts run concurrently; `valid_token()` proves the BFF
/// can still mint a Management token (no new Zitadel surface beyond apps search).
async fn stats(_op: Operator, State(st): State<AppState>) -> Json<Value> {
    let (humans, machines, roles, grants, apps) = tokio::join!(
        st.zitadel.count_humans(),
        st.zitadel.count_machines(),
        st.zitadel.count_roles(),
        st.zitadel.count_grants(),
        st.zitadel.count_apps(),
    );
    let token_healthy = st.zitadel.valid_token().await.is_ok();
    let body = StatsResponse { humans, machines, roles, grants, apps, token_healthy };
    Json(serde_json::to_value(body).unwrap_or_else(|_| json!({})))
}
```

- [ ] **Step 4: Run it, expect PASS.** `cargo test -p llm-chat-admin-api --lib api::contract_tests::stats_response_serializes_camelcase` — `test result: ok. 1 passed`. Then `cargo test -p llm-chat-admin-api --lib` to confirm the whole crate is green.

- [ ] **Step 5: Commit.**

```
git add admin-api/src/api/mod.rs
git commit -m "feat(admin-api): GET /api/stats — Operator-gated dashboard counts

Fan out count_humans/machines/roles/grants/apps concurrently + a valid_token()
health probe; StatsResponse serializes camelCase with null per-count degradation
so one failing fan-out never blanks the dashboard (design §10/§12).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4.5: `Stats` frontend type

- [ ] **Step 1: Write the failing test.** Extend `admin-web/__tests__/api.test.ts` with a type-shape assertion that parses a `/api/stats` body into `Stats` (the import is the failing signal):

```typescript
import type { Stats } from "../lib/types";

describe("stats type", () => {
  it("parses a /api/stats body into Stats (null counts allowed)", () => {
    const body: Stats = {
      humans: 18, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true,
    };
    expect(body.humans).toBe(18);
    const degraded: Stats = {
      humans: null, machines: null, roles: null, grants: null, apps: null,
      tokenHealthy: false,
    };
    expect(degraded.apps).toBeNull();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL.** `pnpm --dir admin-web test api` — fails: `Module '"../lib/types"' has no exported member 'Stats'.`

- [ ] **Step 3: Implement.** Append to `admin-web/lib/types.ts`:

```typescript
export interface Stats {
  humans: number | null;
  machines: number | null;
  roles: number | null;
  grants: number | null;
  apps: number | null;
  tokenHealthy: boolean;
}
```

- [ ] **Step 4: Run it, expect PASS.** `pnpm --dir admin-web test api` — the `stats type` suite passes.

- [ ] **Step 5: Commit.**

```
git add admin-web/lib/types.ts admin-web/__tests__/api.test.ts
git commit -m "feat(admin-web): Stats type mirroring the /api/stats BFF JSON

Counts are number|null (em-dash on null) + tokenHealthy; matches the camelCase
StatsResponse the handler emits (design §10).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4.6: Dashboard page — colourful, deep-linking stat cards

A thin client page mirroring `users/page.tsx`'s `useCallback load()` + `api.get` into `useState` + `ApiError` 401-swallow pattern exactly. Each card is a shadcn `Card` wrapped in a `next/link` to its section, using the mockup's exact icon-tint colours (indigo/blue/cyan/violet) and lucide icons from the spec's nav set. A `null` count renders an em-dash (`—`).

- [ ] **Step 1: Write the failing test.** Create `admin-web/__tests__/dashboard.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import DashboardPage from "../app/(dash)/dashboard/page";
import { api } from "../lib/api";
import type { Stats } from "../lib/types";

vi.mock("next/link", () => ({
  default: ({ href, children }: { href: string; children: React.ReactNode }) => (
    <a href={href}>{children}</a>
  ),
}));

beforeEach(() => {
  Object.defineProperty(window, "location", {
    value: { assign: vi.fn(), href: "" }, writable: true,
  });
});
afterEach(() => vi.restoreAllMocks());

function stub(stats: Stats) {
  vi.spyOn(api, "get").mockResolvedValue(stats as never);
}

describe("dashboard cards", () => {
  it("renders each count and deep-links into its section", async () => {
    stub({ humans: 18, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    expect(await screen.findByText("18")).toBeInTheDocument();
    expect(screen.getByText("Humans")).toBeInTheDocument();
    expect(screen.getByText("Machine accounts")).toBeInTheDocument();
    // each card deep-links to its area
    expect(screen.getByRole("link", { name: /Humans/ })).toHaveAttribute("href", "/users");
    expect(screen.getByRole("link", { name: /Apps/ })).toHaveAttribute("href", "/apps");
    expect(screen.getByRole("link", { name: /Roles/ })).toHaveAttribute("href", "/roles");
  });

  it("shows an em-dash for a failed (null) count", async () => {
    stub({ humans: null, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    expect(await screen.findByText("—")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run it, expect FAIL.** `pnpm --dir admin-web test dashboard` — fails: `Failed to resolve import "../app/(dash)/dashboard/page"`.

- [ ] **Step 3: Implement.** Create `admin-web/app/(dash)/dashboard/page.tsx`:

```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { Users, UserRound, Bot, ShieldCheck, KeyRound, AppWindow } from "lucide-react";
import { toast } from "sonner";
import { Card } from "@/components/ui/card";
import { api, ApiError } from "@/lib/api";
import type { Stats } from "@/lib/types";

// Mockup tints (docs/superpowers/specs/mockups/console-shell.html): each card's
// icon sits on a translucent brand wash. tone = [icon bg, icon fg].
type Card = {
  key: keyof Omit<Stats, "tokenHealthy">;
  label: string;
  href: string;
  Icon: typeof Users;
  bg: string;
  fg: string;
};

const CARDS: Card[] = [
  { key: "humans",   label: "Humans",           href: "/users",  Icon: UserRound,  bg: "bg-blue-500/12",   fg: "text-blue-600" },
  { key: "machines", label: "Machine accounts", href: "/users",  Icon: Bot,        bg: "bg-cyan-500/14",   fg: "text-cyan-600" },
  { key: "roles",    label: "Roles",            href: "/roles",  Icon: ShieldCheck, bg: "bg-indigo-500/12", fg: "text-indigo-600" },
  { key: "grants",   label: "Grants",           href: "/users",  Icon: KeyRound,   bg: "bg-emerald-500/12", fg: "text-emerald-600" },
  { key: "apps",     label: "Apps",             href: "/apps",   Icon: AppWindow,  bg: "bg-violet-500/14",  fg: "text-violet-600" },
];

export default function DashboardPage() {
  const [stats, setStats] = useState<Stats | null>(null);

  const load = useCallback(async () => {
    try {
      setStats(await api.get<Stats>("/api/stats"));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load dashboard");
      }
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  // null count (its fan-out failed) -> em-dash, never a misleading 0 (§12).
  const show = (n: number | null | undefined) => (n == null ? "—" : String(n));

  return (
    <main className="container mx-auto py-8 space-y-6">
      <div>
        <h1 className="text-2xl font-semibold">Dashboard</h1>
        <p className="text-sm text-muted-foreground">
          People, roles, and apps across every app on the platform.
        </p>
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-5">
        {CARDS.map(({ key, label, href, Icon, bg, fg }) => (
          <Link key={key + label} href={href} aria-label={label}
            className="transition-shadow hover:shadow-md rounded-xl">
            <Card className="p-4">
              <div className={`mb-3 flex h-10 w-10 items-center justify-center rounded-xl ${bg} ${fg}`}>
                <Icon className="h-5 w-5" />
              </div>
              <div className="text-2xl font-bold tracking-tight">
                {stats ? show(stats[key]) : "—"}
              </div>
              <div className="text-sm text-muted-foreground">{label}</div>
            </Card>
          </Link>
        ))}
      </div>
      {stats && !stats.tokenHealthy && (
        <p className="text-sm text-rose-600">
          Service-account token unavailable — counts may be stale.
        </p>
      )}
    </main>
  );
}
```

  Note: the local `Card` *type* shadows the imported `Card` *component* name — rename the type to avoid the collision. Use `CardDef` instead:

```tsx
type CardDef = {
  key: keyof Omit<Stats, "tokenHealthy">;
  label: string;
  href: string;
  Icon: typeof Users;
  bg: string;
  fg: string;
};

const CARDS: CardDef[] = [
```

  (Keep the `Users` import — it anchors the `typeof Users` icon type even though it is not rendered directly. If lint flags it as unused, switch `Icon: typeof Users` to `Icon: typeof UserRound` and drop the `Users` import.)

- [ ] **Step 4: Run it, expect PASS.** `pnpm --dir admin-web test dashboard` — both `dashboard cards` specs pass.

- [ ] **Step 5: Commit.**

```
git add "admin-web/app/(dash)/dashboard/page.tsx" admin-web/__tests__/dashboard.test.tsx
git commit -m "feat(admin-web): Dashboard page — colourful stat cards that deep-link

Mirrors users/page.tsx load() + ApiError 401-swallow; five Card tiles (mockup
tints) link into Users/Roles/Apps; null counts render an em-dash, not 0 (§10).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4.7: Make Dashboard the default landing

The `(dash)` route group adds no URL segment (Next.js 16, §4), so `app/(dash)/page.tsx` is the index for `/`. It server-redirects to `/dashboard`.

- [ ] **Step 1: Write the failing test.** Add to `admin-web/__tests__/dashboard.test.tsx` a redirect assertion that mocks `next/navigation`:

```tsx
describe("dash index landing", () => {
  it("redirects / to /dashboard", async () => {
    const redirect = vi.fn();
    vi.doMock("next/navigation", () => ({ redirect }));
    const { default: DashIndex } = await import("../app/(dash)/page");
    DashIndex();
    expect(redirect).toHaveBeenCalledWith("/dashboard");
  });
});
```

- [ ] **Step 2: Run it, expect FAIL.** `pnpm --dir admin-web test dashboard` — the new spec fails: `Failed to resolve import "../app/(dash)/page"`.

- [ ] **Step 3: Implement.** Read `node_modules/next/dist/docs/` for the App-Router `redirect` API first (AGENTS.md mandate), then create `admin-web/app/(dash)/page.tsx`:

```tsx
import { redirect } from "next/navigation";

// The (dash) route group adds no URL segment, so this is the index for "/".
// The Console lands on the Dashboard (design §10).
export default function DashIndex() {
  redirect("/dashboard");
}
```

- [ ] **Step 4: Run it, expect PASS.** `pnpm --dir admin-web test dashboard` — all three suites (`dashboard cards`, `dash index landing`) pass.

- [ ] **Step 5: Commit.**

```
git add "admin-web/app/(dash)/page.tsx" admin-web/__tests__/dashboard.test.tsx
git commit -m "feat(admin-web): land the Console on the Dashboard

(dash) group adds no URL segment (Next 16, design §4); the / index redirects to
/dashboard so the Console opens on the stat cards.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4.8: Playwright cards-render check (ADMIN_IT-gated)

Extend the existing authenticated-operator describe block in `e2e/smoke.spec.ts` with a Dashboard assertion, reusing the existing `ADMIN_IT=1` login (no new login plumbing). It navigates to the landing, confirms the redirect to `/dashboard`, and asserts the cards render against the live `/api/stats`.

- [ ] **Step 1: Write the failing test.** Add a test inside the existing `test.describe("authenticated operator flow", ...)` block in `admin-web/e2e/smoke.spec.ts` (after the `login -> list users` test):

```typescript
  test("dashboard is the landing and renders stat cards", async ({ page }) => {
    // Reuse the operator session established by the login test's storage; if run
    // standalone, log in first (same field names as the users test).
    await page.goto("/login");
    await page.locator('input[name="loginName"]').fill(process.env.ADMIN_IT_USER!);
    await page.getByRole("button", { name: /next|continue/i }).click();
    await page.locator('input[name="password"]').fill(process.env.ADMIN_IT_PASS!);
    await page.getByRole("button", { name: /next|continue|sign in/i }).click();
    const skip2fa = page.getByRole("button", { name: /skip/i });
    await skip2fa.waitFor({ state: "visible", timeout: 8000 })
      .then(() => skip2fa.click()).catch(() => {});

    // The Console lands on /dashboard (design §10); / redirects there.
    await page.goto("/");
    await page.waitForURL(/\/dashboard/);
    await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();

    // Cards render against the live /api/stats fan-out: labels are always present,
    // and each count is either a number or an em-dash (never blank).
    await expect(page.getByText("Humans")).toBeVisible();
    await expect(page.getByText("Apps")).toBeVisible();
    const humansLink = page.getByRole("link", { name: /Humans/ });
    await expect(humansLink).toHaveAttribute("href", "/users");
  });
```

- [ ] **Step 2: Run it, expect FAIL.** Without the stack: `pnpm --dir admin-web e2e -g "dashboard is the landing"` — the test is `skip`ped (the describe's `test.skip(!FULL, ...)`), so it reports `skipped`, not pass. With the stack up (`ADMIN_IT=1 ADMIN_IT_USER=… ADMIN_IT_PASS=… pnpm --dir admin-web e2e -g "dashboard is the landing"`) **before** the page exists, it fails: the heading `Dashboard` is never visible (timeout).

- [ ] **Step 3: Implement.** No new app code — Tasks 4.6/4.7 already shipped the page + redirect. This step only adds the e2e spec above (already written in Step 1). Confirm the spec file's import line still reads `import { test, expect } from "@playwright/test";` (unchanged).

- [ ] **Step 4: Run it, expect PASS.** With the stack up: `ADMIN_IT=1 ADMIN_IT_USER=<op> ADMIN_IT_PASS=<pw> ADMIN_WEB_URL=http://localhost:3000 pnpm --dir admin-web e2e -g "dashboard is the landing"` — `1 passed`. (Offline it stays `skipped`, matching the existing smoke contract.)

- [ ] **Step 5: Commit.**

```
git add admin-web/e2e/smoke.spec.ts
git commit -m "test(admin-web): e2e — dashboard is the landing + cards render

ADMIN_IT-gated: / redirects to /dashboard, the Dashboard heading + stat cards
render against the live /api/stats fan-out, and a card deep-links to /users.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

**Phase 4 done when:** `cargo test -p llm-chat-admin-api --lib` is green (count helper both-forms + camelCase contract), `pnpm --dir admin-web test` is green (Stats type, card render, em-dash degradation, redirect), and — with the stack up — the `ADMIN_IT` Playwright check shows the Console landing on the Dashboard with live counts. `/api/stats` is one `Operator`-gated handler over five existing `_search` endpoints plus `valid_token()`; no new Zitadel surface beyond apps `_search` (Phase 2).

## Phase 5: Audit (capability-gated)

Spec §11 + §3 (Audit needs `IAM_OWNER_VIEWER`, more than `ORG_OWNER`) and §14 risk #1. This phase ships the Audit screen and its BFF surface **fail-closed**: a `GET /api/capabilities` probe tells the UI whether the SA can actually read the event log; if not, the page shows the banner *"Audit requires IAM_OWNER_VIEWER on the service account"* instead of erroring. When the capability is present, events are listed in a searchable `DataTable`, **confined by `resourceOwner` to the SA's own org** (the `/admin/v1/events/_search` log is instance-wide — never leak other orgs).

**Prerequisites (Phase 0 — assume already merged):** the `(dash)/layout.tsx` shell + `components/shell/*` with the `NAV` array carrying the `audit` entry (`ScrollText` icon, `href: "/audit"`), and the `components/ui/data-table.tsx` `emptyMessage` prop fix (§4). This phase consumes those; it does not re-create them.

**Files:**
- `admin-api/src/zitadel/events.rs` (Create) — `ZitadelClient` methods: `sa_org_id()` (resolve the SA's org via `/auth/v1/users/me`), `search_events(...)` over `POST /admin/v1/events/_search` with sequence-cursor paging + `resourceOwner` confinement, and `can_read_events()` capability probe.
- `admin-api/src/zitadel/mod.rs` (Modify) — add `pub mod events;`.
- `admin-api/src/api/mod.rs` (Modify) — add `GET /api/events` (with editor/aggregate/from/asc filters) and `GET /api/capabilities` handlers + routes; their camelCase/passthrough contract tests.
- `admin-web/lib/types.ts` (Modify) — add `Capabilities`, `AuditEvent`, `EventList` interfaces (the audit JSON contract).
- `admin-web/components/audit/columns.tsx` (Create) — `auditColumns`: editor / aggregate / event-type / date `ColumnDef<AuditEvent>[]`.
- `admin-web/app/(dash)/audit/page.tsx` (Create) — the page: probe capabilities; render the fail-closed banner OR the searchable events `DataTable`.
- `admin-web/e2e/smoke.spec.ts` (Modify) — Playwright test asserting the capability-false banner renders.

---

### Task 5.1: `sa_org_id()` — resolve the SA's own org (confinement anchor)

- [ ] **Step 1: Write the failing test.** Append to a new `admin-api/src/zitadel/events.rs` a pure-parse unit test for the org-id extraction helper (the network call is integration-tested under `ADMIN_IT`; the parse shape is the part we lock down here):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn org_id_from_me_reads_details_resource_owner() {
        // GET /auth/v1/users/me shape (provision.py:fetch_org_id, §11 confinement).
        let body = json!({
            "user": { "details": { "resourceOwner": "org-123" } }
        });
        assert_eq!(org_id_from_me(&body), Some("org-123".to_string()));
    }

    #[test]
    fn org_id_from_me_is_none_when_missing() {
        assert_eq!(org_id_from_me(&json!({ "user": { "details": {} } })), None);
    }
}
```

- [ ] **Step 2: Run it, expect FAIL.** Command: `cargo test -p llm-chat-admin-api --lib zitadel::events`. Expected: compile error `cannot find function 'org_id_from_me' in this scope` (module body not yet written).

- [ ] **Step 3: Implement.** Create `admin-api/src/zitadel/events.rs` with the pure helper + the `sa_org_id` method (the test block above goes at the bottom of this same file):

```rust
//! Admin-API event log (audit) — POST /admin/v1/events/_search (design §11).
//! CAPABILITY-GATED: the event log needs IAM_OWNER_VIEWER (instance), which the
//! ORG_OWNER service account does NOT have, so `can_read_events` probes it and
//! the UI fails closed (§3/§11). When readable, results are CONFINED by
//! `resourceOwner` to the SA's own org — the instance log is instance-wide and
//! must never leak other orgs' events (fail-closed confinement, §11).

use serde_json::{json, Value};

use super::error::ZitadelError;
use super::ZitadelClient;

/// PURE: extract the SA's org id from a GET /auth/v1/users/me body. The org is
/// at user.details.resourceOwner (provision.py:fetch_org_id, §11).
pub fn org_id_from_me(body: &Value) -> Option<String> {
    body.get("user")
        .and_then(|u| u.get("details"))
        .and_then(|d| d.get("resourceOwner"))
        .and_then(Value::as_str)
        .map(String::from)
}

impl ZitadelClient {
    /// Resolve the SA's own org id (the confinement anchor). FAIL CLOSED: if the
    /// org cannot be resolved we return NotFound rather than search unconfined,
    /// because an unconfined event search would leak every org (§11).
    pub async fn sa_org_id(&self) -> Result<String, ZitadelError> {
        let url = format!("{}/auth/v1/users/me", self.cfg.issuer);
        let v = self.get_json(&url).await?;
        org_id_from_me(&v).ok_or(ZitadelError::NotFound)
    }
}
```

- [ ] **Step 4: Run it, expect PASS.** Command: `cargo test -p llm-chat-admin-api --lib zitadel::events`. Expected: `test result: ok. 2 passed`.

- [ ] **Step 5: Commit.**
```
git add admin-api/src/zitadel/events.rs
git commit -m "feat(admin-api): events.rs — sa_org_id() confinement anchor for audit

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.2: `search_events()` — confined sequence-cursor paging

- [ ] **Step 1: Write the failing test.** Add to the `tests` module in `admin-api/src/zitadel/events.rs` a pure test for the request-body builder (the part with the confinement + cursor logic; the HTTP round-trip is `ADMIN_IT`-gated):

```rust
    #[test]
    fn events_body_confines_to_org_and_carries_filters() {
        let b = build_events_body(
            "org-123",
            &EventQuery {
                editor_user_id: Some("u-9".into()),
                aggregate_id: Some("agg-7".into()),
                from: Some("2026-06-01T00:00:00Z".into()),
                asc: false,
                limit: 50,
            },
        );
        assert_eq!(b["asc"], json!(false));
        assert_eq!(b["limit"], json!(50));
        // resourceOwner confinement is ALWAYS present (§11).
        assert_eq!(b["resourceOwner"], json!("org-123"));
        assert_eq!(b["editorUserId"], json!("u-9"));
        assert_eq!(b["aggregateId"], json!(["agg-7"]));
        assert_eq!(b["creationDate"], json!("2026-06-01T00:00:00Z"));
    }

    #[test]
    fn events_body_omits_absent_filters_but_keeps_confinement() {
        let b = build_events_body(
            "org-123",
            &EventQuery { editor_user_id: None, aggregate_id: None, from: None, asc: false, limit: 100 },
        );
        assert_eq!(b["resourceOwner"], json!("org-123"));
        assert!(b.get("editorUserId").is_none());
        assert!(b.get("aggregateId").is_none());
        assert!(b.get("creationDate").is_none());
    }
```

- [ ] **Step 2: Run it, expect FAIL.** Command: `cargo test -p llm-chat-admin-api --lib zitadel::events`. Expected: compile error `cannot find function 'build_events_body'` / `cannot find type 'EventQuery'`.

- [ ] **Step 3: Implement.** Add to `admin-api/src/zitadel/events.rs`, above the `impl ZitadelClient` block, the `EventQuery` struct + pure body builder, and a `search_events` method inside the impl. The `/admin/v1/events/_search` request supports `limit`, `asc`, `editorUserId`, `aggregateId[]`, `creationDate`, and `resourceOwner` (sequence-cursor paging is `asc` + `creationDate` lower bound — grounded in §11):

```rust
/// One audit query (mapped from /api/events query params). Pure input to
/// `build_events_body`; HTTP-agnostic so it is unit-testable.
pub struct EventQuery {
    pub editor_user_id: Option<String>,
    pub aggregate_id: Option<String>,
    /// Lower-bound creationDate (RFC3339) — the sequence cursor for paging.
    pub from: Option<String>,
    pub asc: bool,
    pub limit: u32,
}

/// PURE: build the POST /admin/v1/events/_search body. `resourceOwner` is ALWAYS
/// set to the SA's org so the instance-wide log is confined to one org (§11);
/// absent filters are omitted (no guessed defaults), present ones mapped to the
/// exact admin-API field names.
pub fn build_events_body(org_id: &str, q: &EventQuery) -> Value {
    let mut body = json!({
        "limit": q.limit,
        "asc": q.asc,
        "resourceOwner": org_id,
    });
    let obj = body.as_object_mut().expect("object");
    if let Some(e) = q.editor_user_id.as_ref().filter(|s| !s.is_empty()) {
        obj.insert("editorUserId".into(), json!(e));
    }
    if let Some(a) = q.aggregate_id.as_ref().filter(|s| !s.is_empty()) {
        // aggregateId is a repeated field on the events search.
        obj.insert("aggregateId".into(), json!([a]));
    }
    if let Some(d) = q.from.as_ref().filter(|s| !s.is_empty()) {
        obj.insert("creationDate".into(), json!(d));
    }
    body
}
```

Then add this method inside the existing `impl ZitadelClient` block (after `sa_org_id`):

```rust
    /// Search the audit log: POST /admin/v1/events/_search, CONFINED to the SA's
    /// org via resourceOwner (§11). Returns the `events` array passed through
    /// (camelCase preserved). Needs IAM_OWNER_VIEWER — gate via can_read_events.
    pub async fn search_events(&self, q: &EventQuery) -> Result<Vec<Value>, ZitadelError> {
        let org_id = self.sa_org_id().await?;
        let url = format!("{}/admin/v1/events/_search", self.cfg.issuer);
        let v = self.post_json(&url, &build_events_body(&org_id, q)).await?;
        Ok(v.get("events").and_then(Value::as_array).cloned().unwrap_or_default())
    }
```

- [ ] **Step 4: Run it, expect PASS.** Command: `cargo test -p llm-chat-admin-api --lib zitadel::events`. Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Commit.**
```
git add admin-api/src/zitadel/events.rs
git commit -m "feat(admin-api): search_events — resourceOwner-confined event search (§11)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.3: `can_read_events()` — fail-closed capability probe

- [ ] **Step 1: Write the failing test.** Add to the `tests` module in `admin-api/src/zitadel/events.rs` a pure test for the error-to-capability classifier (a `Forbidden`/`NotFound` upstream means *no capability*; any other error is a real failure and must propagate, not silently read as "no capability"):

```rust
    #[test]
    fn forbidden_and_not_found_mean_no_capability() {
        assert_eq!(capability_from(Err(ZitadelError::Forbidden)), Ok(false));
        assert_eq!(capability_from(Err(ZitadelError::NotFound)), Ok(false));
    }

    #[test]
    fn ok_means_capability_present() {
        assert_eq!(capability_from(Ok(())), Ok(true));
    }

    #[test]
    fn other_errors_propagate_not_swallowed() {
        // A transport/upstream failure is NOT "no capability" — surface it.
        assert_eq!(
            capability_from(Err(ZitadelError::Upstream)),
            Err(ZitadelError::Upstream)
        );
    }
```

- [ ] **Step 2: Run it, expect FAIL.** Command: `cargo test -p llm-chat-admin-api --lib zitadel::events`. Expected: compile error `cannot find function 'capability_from'`.

- [ ] **Step 3: Implement.** Add to `admin-api/src/zitadel/events.rs` the pure classifier (above `impl ZitadelClient`) and the `can_read_events` probe method (inside the impl). The probe does the cheapest possible confined search (`limit: 1`) and maps the auth error to `false`:

```rust
/// PURE: classify an events probe result into a capability boolean. Only the
/// permission errors (Forbidden = missing IAM_OWNER_VIEWER, NotFound = endpoint
/// unavailable) mean "no capability"; everything else is a genuine failure and
/// must propagate so we never report "unavailable" for a transient outage (§11).
pub fn capability_from(res: Result<(), ZitadelError>) -> Result<bool, ZitadelError> {
    match res {
        Ok(()) => Ok(true),
        Err(ZitadelError::Forbidden) | Err(ZitadelError::NotFound) => Ok(false),
        Err(other) => Err(other),
    }
}
```

Then add the method inside `impl ZitadelClient`:

```rust
    /// Probe whether the SA can read the event log (needs IAM_OWNER_VIEWER, §3).
    /// Does a minimal confined search and maps a 403/404 to `false`; other errors
    /// propagate (do not masquerade as "no capability").
    pub async fn can_read_events(&self) -> Result<bool, ZitadelError> {
        let probe = EventQuery {
            editor_user_id: None, aggregate_id: None, from: None, asc: false, limit: 1,
        };
        capability_from(self.search_events(&probe).await.map(|_| ()))
    }
```

- [ ] **Step 4: Run it, expect PASS.** Command: `cargo test -p llm-chat-admin-api --lib zitadel::events`. Expected: `test result: ok. 7 passed`.

- [ ] **Step 5: Commit.**
```
git add admin-api/src/zitadel/events.rs
git commit -m "feat(admin-api): can_read_events probe — fail-closed audit capability (§3/§11)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.4: Wire `events` into the Zitadel module

- [ ] **Step 1: Write the failing test.** No new test file — the wiring is proven by the whole crate compiling. The guard is the build itself; assert it currently fails to see `events`:

```rust
// (no code — this task's "test" is `cargo build` resolving the module path)
```

- [ ] **Step 2: Run it, expect FAIL.** Command: `cargo build -p llm-chat-admin-api`. Expected at this point (before the edit, if `events.rs` is not yet declared): the new file is not part of the crate and its `impl ZitadelClient` methods are unreachable — `cargo build` succeeds but `cargo test --lib zitadel::events` reports `0 tests` / `no test target`. Run `cargo test -p llm-chat-admin-api --lib zitadel::events::tests::ok_means_capability_present` and expect `error: no test named ...` until the module is declared.

- [ ] **Step 3: Implement.** Add the module declaration to `admin-api/src/zitadel/mod.rs`, keeping the existing alphabetical grouping (insert `events` between `error` and `grants`):

```rust
pub mod error;
pub mod events;
pub mod grants;
pub mod keys;
pub mod model;
pub mod token;
pub mod users;
```

- [ ] **Step 4: Run it, expect PASS.** Command: `cargo test -p llm-chat-admin-api --lib zitadel::events`. Expected: `test result: ok. 7 passed` (the module is now compiled and discoverable).

- [ ] **Step 5: Commit.**
```
git add admin-api/src/zitadel/mod.rs
git commit -m "feat(admin-api): register zitadel::events module

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.5: `GET /api/capabilities` + `GET /api/events` handlers

- [ ] **Step 1: Write the failing test.** Add to the `contract_tests` module at the bottom of `admin-api/src/api/mod.rs` a test that the new `EventListQuery` deserializes the audit query params and that the capabilities JSON is the expected shape:

```rust
    #[test]
    fn event_list_query_parses_filters_with_defaults() {
        // editor/aggregate/from optional; asc + limit defaulted when absent.
        let q: EventListQuery = serde_urlencoded::from_str(
            "editor=u-9&aggregate=agg-7&from=2026-06-01T00:00:00Z"
        ).expect("parse event query");
        assert_eq!(q.editor.as_deref(), Some("u-9"));
        assert_eq!(q.aggregate.as_deref(), Some("agg-7"));
        assert_eq!(q.from.as_deref(), Some("2026-06-01T00:00:00Z"));
        assert!(!q.asc, "asc defaults to false (newest-first)");
        assert_eq!(q.limit, 100, "limit defaults to 100");
    }

    #[test]
    fn capabilities_payload_shape() {
        // The /api/capabilities body the audit page reads.
        let v = capabilities_json(false);
        assert_eq!(v.get("events").and_then(Value::as_bool), Some(false));
        assert_eq!(capabilities_json(true).get("events").and_then(Value::as_bool), Some(true));
    }
```

- [ ] **Step 2: Run it, expect FAIL.** Command: `cargo test -p llm-chat-admin-api --lib api::mod::contract_tests`. Expected: compile error `cannot find type 'EventListQuery'` / `cannot find function 'capabilities_json'`.

- [ ] **Step 3: Implement.** In `admin-api/src/api/mod.rs`:

(a) Register the two routes inside `router(...)`, after the `list_roles` route:

```rust
        .route("/api/roles", get(list_roles))
        .route("/api/events", get(list_events))
        .route("/api/capabilities", get(list_capabilities))
```

(b) Add the query struct, the pure `capabilities_json` helper, and the two handlers (place them after `list_roles`):

```rust
#[derive(Deserialize)]
struct EventListQuery {
    editor: Option<String>,
    aggregate: Option<String>,
    from: Option<String>,
    #[serde(default)]
    asc: bool,
    #[serde(default = "default_event_limit")]
    limit: u32,
}
fn default_event_limit() -> u32 { 100 }

/// PURE: the /api/capabilities body. One field today (events); a fail-closed
/// boolean the audit page branches on (§11).
fn capabilities_json(events: bool) -> Value {
    json!({ "events": events })
}

async fn list_capabilities(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let events = st.zitadel.can_read_events().await?;
    Ok(Json(capabilities_json(events)))
}

async fn list_events(_op: Operator, State(st): State<AppState>, Query(qp): Query<EventListQuery>)
    -> Result<Json<Value>, ApiError> {
    let q = crate::zitadel::events::EventQuery {
        editor_user_id: qp.editor,
        aggregate_id: qp.aggregate,
        from: qp.from,
        asc: qp.asc,
        limit: qp.limit,
    };
    let events = st.zitadel.search_events(&q).await?;
    Ok(Json(json!({ "result": events })))
}
```

(c) The contract test uses `serde_urlencoded` (already a transitive dep of axum) — add it under `[dev-dependencies]` in `admin-api/Cargo.toml` if it is not already a direct dev-dep:

```toml
[dev-dependencies]
serde_urlencoded = "0.7"
```

- [ ] **Step 4: Run it, expect PASS.** Command: `cargo test -p llm-chat-admin-api --lib api`. Expected: the new tests plus the existing `create_human_accepts_camelcase` etc. all pass — `test result: ok.`

- [ ] **Step 5: Commit.**
```
git add admin-api/src/api/mod.rs admin-api/Cargo.toml
git commit -m "feat(admin-api): GET /api/events + /api/capabilities (audit, §11)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.6: Frontend audit types + columns

- [ ] **Step 1: Write the failing test.** The frontend type layer is type-checked, not unit-tested; the failing "test" is `tsc`. Author the column file referencing not-yet-existing types so the typecheck fails. First add the test driver — a quick compile probe via the e2e/lint pass is overkill; instead assert the columns module typechecks against the new types by writing the columns file (Step 3) and running `pnpm exec tsc --noEmit` (Step 2 will fail until types exist).

```ts
// no standalone unit test — the contract is the TypeScript compile (Step 2/4).
```

- [ ] **Step 2: Run it, expect FAIL.** Command (from `admin-web/`): `pnpm exec tsc --noEmit`. Expected: `error TS2305: Module '"@/lib/types"' has no exported member 'AuditEvent'` (and `Capabilities`, `EventList`) once `components/audit/columns.tsx` exists; before the columns file exists, expected: the audit page (Task 5.7) cannot import `auditColumns`. Run this after Step 3's edits are staged incrementally — here it fails on the missing exports.

- [ ] **Step 3: Implement.**

(a) Append to `admin-web/lib/types.ts` the audit contract (mirrors the camelCase passthrough from `/admin/v1/events/_search` — `editor`, `aggregate`, `type`, `creationDate` are the Zitadel event fields):

```ts
export interface Capabilities {
  events: boolean;
}

export interface AuditEvent {
  sequence?: string;
  creationDate?: string;
  type?: { type?: string; localized?: { localizedMessage?: string } };
  editor?: { userId?: string; displayName?: string };
  aggregate?: { id?: string; type?: string };
}

export interface EventList {
  result: AuditEvent[];
}
```

(b) Create `admin-web/components/audit/columns.tsx` (mirrors `components/users/columns.tsx`'s `buildColumns` shape — a typed `ColumnDef[]` with `Badge` cells; read-only, so no actions menu):

```tsx
"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { Badge } from "@/components/ui/badge";
import type { AuditEvent } from "@/lib/types";

export const auditColumns: ColumnDef<AuditEvent>[] = [
  {
    id: "editor",
    accessorFn: (e) => e.editor?.displayName ?? e.editor?.userId ?? "—",
    header: "Editor",
  },
  {
    id: "eventType",
    accessorFn: (e) => e.type?.localized?.localizedMessage ?? e.type?.type ?? "—",
    header: "Event",
    cell: ({ getValue }) => <Badge variant="secondary">{getValue<string>()}</Badge>,
  },
  {
    id: "aggregate",
    accessorFn: (e) =>
      e.aggregate?.type ? `${e.aggregate.type}/${e.aggregate.id ?? ""}` : (e.aggregate?.id ?? "—"),
    header: "Aggregate",
  },
  {
    id: "creationDate",
    accessorFn: (e) => e.creationDate ?? "",
    header: "Date",
    cell: ({ getValue }) => {
      const raw = getValue<string>();
      return raw ? new Date(raw).toLocaleString() : "—";
    },
  },
];
```

- [ ] **Step 4: Run it, expect PASS.** Command (from `admin-web/`): `pnpm exec tsc --noEmit`. Expected: no errors (exit 0) — `Capabilities`/`AuditEvent`/`EventList` resolve and `auditColumns` typechecks.

- [ ] **Step 5: Commit.**
```
git add admin-web/lib/types.ts admin-web/components/audit/columns.tsx
git commit -m "feat(admin-web): audit event types + read-only columns

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.7: Audit page — fail-closed banner OR searchable events table

- [ ] **Step 1: Write the failing test.** Extend `admin-web/e2e/smoke.spec.ts` with a route-mocked test that forces `events:false` and asserts the banner. It mocks `/api/me` and `/api/capabilities` so it runs WITHOUT the full stack (no `ADMIN_IT` gate), proving the fail-closed branch deterministically:

```ts
test("audit page fails closed: capabilities.events=false shows the IAM_OWNER_VIEWER banner", async ({ page }) => {
  // Force the capability probe to report "no events" — the page must not error,
  // it must show the fail-closed banner (design §11).
  await page.route("**/api/me", (r) =>
    r.fulfill({ json: { userId: "op-1", name: "operator", roles: ["chat.admin"] } }),
  );
  await page.route("**/api/capabilities", (r) => r.fulfill({ json: { events: false } }));
  // If the page ever calls /api/events with the capability off, fail loudly.
  let eventsCalled = false;
  await page.route("**/api/events*", (r) => {
    eventsCalled = true;
    return r.fulfill({ json: { result: [] } });
  });

  await page.goto("/audit");
  await expect(
    page.getByText("Audit requires IAM_OWNER_VIEWER on the service account"),
  ).toBeVisible();
  expect(eventsCalled, "must not fetch events when capability is false").toBe(false);
});
```

- [ ] **Step 2: Run it, expect FAIL.** Command (from `admin-web/`): `pnpm exec playwright test e2e/smoke.spec.ts -g "fails closed"`. Expected: the test fails because `/audit` 404s (no page yet) — `expect(...).toBeVisible()` times out / `page.goto` lands on a Next.js 404, banner text never appears.

- [ ] **Step 3: Implement.** Create `admin-web/app/(dash)/audit/page.tsx`. It mirrors `users/page.tsx` exactly (a `useCallback load()` doing `api.get` into `useState`, 401 swallowed because `lib/api` redirects), but branches on the capability and only fetches events when `events === true` (fail-closed). The page body is the page slot — the shell (`(dash)/layout.tsx`) supplies the chrome:

```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { auditColumns } from "@/components/audit/columns";
import { api, ApiError } from "@/lib/api";
import type { AuditEvent, Capabilities, EventList } from "@/lib/types";

export default function AuditPage() {
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [events, setEvents] = useState<AuditEvent[]>([]);

  const load = useCallback(async () => {
    try {
      const c = await api.get<Capabilities>("/api/capabilities");
      setCaps(c);
      // FAIL CLOSED: only read the event log when the capability is present (§11).
      if (!c.events) return;
      const list = await api.get<EventList>("/api/events");
      setEvents(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error(e instanceof ApiError ? e.message : "Failed to load audit log");
      }
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <main className="container mx-auto py-8 space-y-4">
      <div>
        <h1 className="text-2xl font-semibold">Audit</h1>
        <p className="text-sm text-muted-foreground">
          Org-scoped event log from Zitadel.
        </p>
      </div>

      {caps && !caps.events ? (
        <div
          role="alert"
          className="rounded-md border border-amber-300 bg-amber-50 p-4 text-sm text-amber-900 dark:border-amber-900/50 dark:bg-amber-950/40 dark:text-amber-200"
        >
          <p className="font-medium">Audit unavailable</p>
          <p>Audit requires IAM_OWNER_VIEWER on the service account.</p>
          <p className="mt-1 text-amber-800/80 dark:text-amber-200/70">
            ORG_OWNER cannot read the instance event log; granting the instance
            role is a separate, explicit decision (design §3/§11).
          </p>
        </div>
      ) : (
        <DataTable
          columns={auditColumns}
          data={events}
          filterColumn="editor"
          filterPlaceholder="Filter by editor..."
          emptyMessage="No events."
        />
      )}
    </main>
  );
}
```

- [ ] **Step 4: Run it, expect PASS.** Command (from `admin-web/`): `pnpm exec playwright test e2e/smoke.spec.ts -g "fails closed"`. Expected: `1 passed` — the banner is visible and `/api/events` was never called.

- [ ] **Step 5: Commit.**
```
git add admin-web/app/(dash)/audit/page.tsx admin-web/e2e/smoke.spec.ts
git commit -m "feat(admin-web): audit page — fail-closed banner + events table (§11)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.8: Capability-on path — events table renders (mocked)

- [ ] **Step 1: Write the failing test.** Add a companion e2e test that forces `events:true`, returns one event, and asserts the row renders through `auditColumns` (proves the non-banner branch wires `/api/events` → `DataTable`):

```ts
test("audit page with capability on lists events", async ({ page }) => {
  await page.route("**/api/me", (r) =>
    r.fulfill({ json: { userId: "op-1", name: "operator", roles: ["chat.admin"] } }),
  );
  await page.route("**/api/capabilities", (r) => r.fulfill({ json: { events: true } }));
  await page.route("**/api/events*", (r) =>
    r.fulfill({
      json: {
        result: [
          {
            sequence: "42",
            creationDate: "2026-06-01T10:00:00Z",
            type: { type: "user.human.added", localized: { localizedMessage: "User added" } },
            editor: { userId: "u-9", displayName: "Operator One" },
            aggregate: { id: "u-9", type: "user" },
          },
        ],
      },
    }),
  );

  await page.goto("/audit");
  await expect(page.getByText("User added")).toBeVisible();
  await expect(page.getByText("Operator One")).toBeVisible();
  // The banner must NOT be present on the capability-on path.
  await expect(
    page.getByText("Audit requires IAM_OWNER_VIEWER on the service account"),
  ).toHaveCount(0);
});
```

- [ ] **Step 2: Run it, expect FAIL.** Command (from `admin-web/`): `pnpm exec playwright test e2e/smoke.spec.ts -g "capability on"`. Expected: this should actually PASS already if Task 5.7's page is correct — so run it first to confirm GREEN-by-construction. If it FAILS, the failure pinpoints a wiring bug (e.g. `filterColumn="editor"` mismatching the column `id`); fix the page, not the test. (TDD note: this task's test guards the second branch the prior task's test didn't cover — a red result here means the on-path is broken.)

- [ ] **Step 3: Implement.** No new production code is expected if Task 5.7 is correct. If Step 2 was red, the root cause is almost always the filter column id — confirm `auditColumns` has a column with `id: "editor"` (it does, Task 5.6) so `DataTable`'s `table.getColumn("editor")` resolves. Make no change unless Step 2 was red.

- [ ] **Step 4: Run it, expect PASS.** Command (from `admin-web/`): `pnpm exec playwright test e2e/smoke.spec.ts -g "capability on"`. Expected: `1 passed`.

- [ ] **Step 5: Commit.**
```
git add admin-web/e2e/smoke.spec.ts
git commit -m "test(admin-web): audit capability-on path renders events (§11)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5.9: Live confinement smoke (ADMIN_IT-gated)

- [ ] **Step 1: Write the failing test.** Append an `ADMIN_IT`-gated integration test to `admin-api/tests/integration.rs` that exercises the real probe + confined search against the running Zitadel. It tolerates either outcome of the capability (the instance grant is a pending decision, §14 #1) but asserts the confinement invariant whenever events ARE readable:

```rust
#[tokio::test]
async fn it_audit_capability_and_confinement() {
    if !it_enabled() {
        eprintln!("ADMIN_IT!=1 — skipping audit capability test");
        return;
    }
    let cfg = llm_chat_admin_api_cfg();
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let z = admin_client(cfg, http);

    // The SA org must resolve (confinement anchor) — this is independent of the
    // event-read grant.
    let org = z.sa_org_id().await.expect("sa_org_id resolves the SA org");
    assert!(!org.is_empty(), "resourceOwner must be non-empty");

    // Capability probe must not panic and returns a definite boolean (§3/§11).
    let can = z.can_read_events().await.expect("capability probe returns Ok");
    if can {
        // When readable, every returned event is confined to the SA's org.
        let q = llm_chat_admin_api::zitadel::events::EventQuery {
            editor_user_id: None, aggregate_id: None, from: None, asc: false, limit: 20,
        };
        let events = z.search_events(&q).await.expect("confined search");
        for e in &events {
            if let Some(owner) = e.get("editor").and_then(|x| x.get("resourceOwner")).and_then(|v| v.as_str()) {
                assert_eq!(owner, org, "event leaked from another org (confinement broken)");
            }
        }
    } else {
        eprintln!("audit capability OFF (IAM_OWNER_VIEWER not granted) — banner path, §14 #1");
    }
}
```

- [ ] **Step 2: Run it, expect FAIL.** Command: `cargo test -p llm-chat-admin-api --test integration it_audit_capability_and_confinement`. Expected (offline, `ADMIN_IT` unset): the test compiles and prints `skipping` then passes trivially — to see a real FAIL, set `ADMIN_IT=1` with the env unconfigured and observe `sa_org_id resolves the SA org` panic. (Offline default is green-by-skip, consistent with the existing integration tests.)

- [ ] **Step 3: Implement.** No production code — the methods exist (Tasks 5.1–5.3). This task only adds the gated test. If Step 2 (under `ADMIN_IT=1`) reveals `sa_org_id` returns `NotFound`, the root cause is the `/auth/v1/users/me` shape drift (§14) — fix `org_id_from_me`'s path against the live body, do not loosen the confinement to "unconfined on missing org".

- [ ] **Step 4: Run it, expect PASS.** Command (full stack): `ADMIN_IT=1 cargo test -p llm-chat-admin-api --test integration it_audit_capability_and_confinement`. Expected: `test result: ok. 1 passed` (capability either branch; confinement invariant holds when on).

- [ ] **Step 5: Commit.**
```
git add admin-api/tests/integration.rs
git commit -m "test(admin-api): ADMIN_IT audit capability + resourceOwner confinement (§11/§14)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```
