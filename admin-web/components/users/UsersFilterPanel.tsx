"use client";
import { Bot, Lock, Plus, Search, User, Users } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { Role } from "@/lib/types";

/** The active filter category. `role:<key>` narrows to holders of one role. */
export type UsersCategory =
  | "all"
  | "humans"
  | "machines"
  | "locked"
  | `role:${string}`;

export interface UsersFilterCounts {
  all: number;
  humans: number;
  machines: number;
  locked: number;
  /** roleKey -> number of users holding it. */
  byRole: Map<string, number>;
}

/** A deterministic swatch color per role key, reusing the avatar hash so the
 * same role always gets the same dot. */
const SWATCH = [
  "bg-indigo-500",
  "bg-emerald-500",
  "bg-blue-500",
  "bg-amber-500",
  "bg-violet-500",
  "bg-cyan-500",
  "bg-rose-500",
] as const;
function swatchFor(key: string): string {
  let hash = 0;
  for (let i = 0; i < key.length; i++) hash = (hash * 31 + key.charCodeAt(i)) | 0;
  return SWATCH[Math.abs(hash) % SWATCH.length];
}

/** One nav row: icon (or swatch) + label + right-aligned count, indigo-tinted
 * when active. Built on the shadcn ghost Button so it inherits focus/hover. */
function NavRow({
  active,
  onClick,
  label,
  count,
  icon,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  count: number;
  icon: React.ReactNode;
}) {
  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={onClick}
      aria-pressed={active}
      className={`h-9 w-full justify-start gap-2.5 px-2.5 font-normal ${
        active
          ? "bg-indigo-500/10 text-indigo-600 hover:bg-indigo-500/10 hover:text-indigo-600"
          : ""
      }`}
    >
      <span className={active ? "text-indigo-600" : "text-muted-foreground"}>
        {icon}
      </span>
      <span className="min-w-0 flex-1 truncate text-left">{label}</span>
      <span
        className={`text-xs tabular-nums ${active ? "text-indigo-600" : "text-muted-foreground"}`}
      >
        {count}
      </span>
    </Button>
  );
}

/**
 * The secondary filter rail to the LEFT of the Users table (mockup `.panel`):
 * a person-icon header with a create "+", a "Filter users…" search, and a nav
 * of categories (All / Humans / Machines / Locked) plus a per-role section.
 * Built entirely from shadcn primitives (Button, Input).
 */
export function UsersFilterPanel({
  query,
  onQueryChange,
  active,
  onActive,
  counts,
  roles,
  onCreate,
}: {
  query: string;
  onQueryChange: (q: string) => void;
  active: UsersCategory;
  onActive: (c: UsersCategory) => void;
  counts: UsersFilterCounts;
  roles: Role[];
  onCreate: () => void;
}) {
  return (
    <aside className="bg-card flex w-[230px] shrink-0 flex-col rounded-xl border shadow-sm">
      {/* Header: person tile + label, create "+" on the right. */}
      <div className="flex items-center justify-between gap-2 border-b px-3 py-2.5">
        <span className="flex items-center gap-2 text-sm font-semibold">
          <span
            aria-hidden
            className="flex size-6 items-center justify-center rounded-md bg-blue-500/12 text-blue-600"
          >
            <Users className="size-3.5" />
          </span>
          Users
        </span>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Create user"
          onClick={onCreate}
        >
          <Plus className="size-4" />
        </Button>
      </div>

      {/* Filter search. */}
      <div className="px-3 pt-3">
        <div className="relative">
          <Search className="text-muted-foreground pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2" />
          <Input
            placeholder="Filter users…"
            value={query}
            onChange={(e) => onQueryChange(e.target.value)}
            className="h-8 pl-8"
            aria-label="Filter users"
          />
        </div>
      </div>

      {/* Category nav + by-role section. */}
      <div className="min-h-0 flex-1 space-y-0.5 overflow-auto px-2 py-2">
        <NavRow
          active={active === "all"}
          onClick={() => onActive("all")}
          label="All users"
          count={counts.all}
          icon={<Users className="size-4" />}
        />
        <NavRow
          active={active === "humans"}
          onClick={() => onActive("humans")}
          label="Humans"
          count={counts.humans}
          icon={<User className="size-4" />}
        />
        <NavRow
          active={active === "machines"}
          onClick={() => onActive("machines")}
          label="Machines"
          count={counts.machines}
          icon={<Bot className="size-4" />}
        />
        <NavRow
          active={active === "locked"}
          onClick={() => onActive("locked")}
          label="Locked"
          count={counts.locked}
          icon={<Lock className="size-4" />}
        />

        {roles.length > 0 && (
          <>
            <div className="text-muted-foreground px-2.5 pt-3 pb-1 text-[10.5px] font-bold tracking-wider uppercase">
              By role
            </div>
            {roles.map((r) => {
              const cat: UsersCategory = `role:${r.key}`;
              return (
                <NavRow
                  key={r.key}
                  active={active === cat}
                  onClick={() => onActive(cat)}
                  label={r.key}
                  count={counts.byRole.get(r.key) ?? 0}
                  icon={
                    <span
                      aria-hidden
                      className={`inline-block size-2.5 rounded-sm ${swatchFor(r.key)}`}
                    />
                  }
                />
              );
            })}
          </>
        )}
      </div>
    </aside>
  );
}
