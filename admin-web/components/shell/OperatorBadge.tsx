"use client";
import { useEffect, useState } from "react";
import { LogOut } from "lucide-react";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/api";
import { initials } from "@/lib/avatar";
import type { Status } from "@/lib/types";

// Re-exported for callers/tests that import `initials` from here; the
// canonical implementation lives in lib/avatar.ts.
export { initials };

function expiresIn(expiresAt: string | null): string {
  if (!expiresAt) return "—";
  const ms = new Date(expiresAt).getTime() - Date.now();
  if (Number.isNaN(ms)) return "—";
  if (ms <= 0) return "expired";
  const mins = Math.floor(ms / 60_000);
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return h > 0 ? `in ${h}h ${m}m` : `in ${m}m`;
}

/** The operator account menu — identity, roles, session expiry + sign out.
 * Two placements (console-shell mockup has both):
 *  - "rail": a white circle avatar pinned to the bottom of the activity-bar
 *    ribbon (the Sidebar); the dropdown opens to the right.
 *  - "bar": the top-right of the top bar as name + gradient avatar (mockup
 *    `.me`); the dropdown opens below.
 * The menu body is identical for both. */
export function OperatorBadge({ variant = "rail" }: { variant?: "rail" | "bar" }) {
  const [status, setStatus] = useState<Status | null>(null);

  useEffect(() => {
    // 401 inside lib/api full-page-redirects to /login; swallow here (spec §4).
    // /api/status carries operator identity AND session expiry in one call.
    api.get<Status>("/api/status").then(setStatus).catch(() => {});
  }, []);

  const op = status?.operator;
  const name = op?.name ?? "—";

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        {variant === "bar" ? (
          // Top bar (mockup `.me`): name + gradient avatar.
          <button
            type="button"
            aria-label="Account menu"
            className="hover:bg-muted flex items-center gap-2 rounded-full py-1 pr-1 pl-3 text-sm font-semibold transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            <span className="max-w-[10rem] truncate">{name}</span>
            <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-linear-to-br from-indigo-500 to-violet-500 text-xs font-bold text-white">
              {initials(name)}
            </span>
          </button>
        ) : (
          // Ribbon (mockup `.avatar`): white circle, violet initials; the name is
          // an sr-only label (accessible name + visible only in the menu).
          <button
            type="button"
            aria-label="Account menu"
            title={`${name} — sign out`}
            className="flex size-[34px] items-center justify-center rounded-full bg-white text-[13px] font-extrabold text-violet-600 shadow-sm transition hover:ring-2 hover:ring-white/50 focus-visible:ring-2 focus-visible:ring-white/70 focus-visible:outline-none"
          >
            {initials(name)}
            <span className="sr-only">{name}</span>
          </button>
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side={variant === "bar" ? "bottom" : "right"}
        align="end"
        sideOffset={variant === "bar" ? 8 : 10}
        className="w-72"
      >
        <DropdownMenuLabel className="flex items-center gap-2.5 font-normal">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-linear-to-br from-indigo-500 to-violet-500 text-xs font-bold text-white">
            {initials(name)}
          </span>
          <span className="min-w-0">
            <span className="block truncate font-semibold">{name}</span>
            <span className="text-muted-foreground block truncate font-mono text-xs">
              {op?.userId ?? "—"}
            </span>
          </span>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <div className="px-2 py-1.5">
          <div className="text-muted-foreground mb-1 text-[11px] font-semibold tracking-wide uppercase">
            Roles
          </div>
          <div className="flex flex-wrap gap-1">
            {op?.roles?.length ? (
              op.roles.map((r) => (
                <Badge key={r} variant={r === "chat.admin" ? "default" : "secondary"}>
                  {r}
                </Badge>
              ))
            ) : (
              <span className="text-muted-foreground text-xs">—</span>
            )}
          </div>
        </div>
        <div className="text-muted-foreground px-2 pb-2 text-xs">
          Session expires{" "}
          <span className="text-foreground">
            {expiresIn(status?.session.expiresAt ?? null)}
          </span>
        </div>
        <DropdownMenuSeparator />
        <DropdownMenuItem asChild>
          <a href="/logout" data-testid="signout" className="cursor-pointer">
            <LogOut className="size-4" />
            Sign out
          </a>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
