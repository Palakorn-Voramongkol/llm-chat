"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { MoreHorizontal } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { avatarGradient, initials } from "@/lib/avatar";
import type { User, UserState } from "@/lib/types";

export type Lifecycle =
  | "deactivate" | "reactivate" | "lock" | "unlock" | "resend-init";

// A user's access grouped by application: the app NAME and the role keys the
// user holds on that app. Preserves per-app grouping (vs. a flat key list).
export interface AppAccess {
  project: string; // application name (resolved from projectId)
  roleKeys: string[];
}

export interface ColumnHandlers {
  onEdit: (u: User) => void;
  onDelete: (u: User) => void;
  onLifecycle: (u: User, action: Lifecycle) => void;
  onGrants: (u: User) => void;
  onAccess: (u: User) => void;
  onKeys: (u: User) => void;
}

// Status dot per semantic token: ACTIVE=success, INITIAL=warning,
// LOCKED/INACTIVE=danger; anything else neutral.
const STATE_DOT: Partial<Record<UserState, string>> = {
  ACTIVE: "bg-success",
  INITIAL: "bg-warning",
  LOCKED: "bg-danger",
  INACTIVE: "bg-danger",
};

// Friendly title-cased status label (vs. the raw Zitadel enum).
const STATE_LABEL: Partial<Record<UserState, string>> = {
  ACTIVE: "Active",
  INACTIVE: "Inactive",
  LOCKED: "Locked",
  INITIAL: "Initial",
  DELETED: "Deleted",
  UNSPECIFIED: "Unspecified",
};

// A user's role chip palette: chat.admin = indigo, chat.user = emerald,
// everything else = slate.
export function roleChipClass(key: string): string {
  if (key === "chat.admin") return "bg-indigo-500/10 text-indigo-600";
  if (key === "chat.user") return "bg-emerald-500/10 text-emerald-600";
  return "bg-slate-500/10 text-slate-600";
}

// The full role-chip className (wrapper + palette) — shared by the Users column
// and the per-user access panel.
export function roleChipFull(key: string): string {
  return `inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium ${roleChipClass(key)}`;
}

export function buildColumns(
  h: ColumnHandlers,
  rolesByUser?: Map<string, AppAccess[]>,
): ColumnDef<User>[] {
  return [
    {
      accessorKey: "userName", header: "User",
      meta: { description: "The account's username (login name) and email. Click a row to open full details." },
      cell: ({ row }) => {
        const u = row.original;
        const display = u.displayName || u.userName;
        return (
          <span className="flex items-center gap-2.5">
            <span
              aria-hidden
              className={`flex size-8 shrink-0 items-center justify-center rounded-full bg-linear-to-br text-xs font-bold text-white ${avatarGradient(u.id || u.userName)}`}
            >
              {initials(display)}
            </span>
            <span className="flex min-w-0 flex-col">
              <span className="truncate font-medium">{u.userName}</span>
              <span className="text-muted-foreground truncate text-xs">
                {u.email || "—"}
              </span>
            </span>
          </span>
        );
      },
    },
    {
      accessorKey: "kind", header: "Type", filterFn: "equalsString",
      meta: { description: "Human (a person who signs in via browser) or Machine (a service account / app server using a key)." },
      cell: ({ row }) => {
        const human = row.original.kind === "Human";
        return (
          <span
            className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${
              human ? "bg-blue-500/10 text-blue-600" : "bg-slate-500/10 text-slate-600"
            }`}
          >
            {row.original.kind}
          </span>
        );
      },
    },
    {
      id: "roles", header: "App access & roles", enableSorting: false,
      meta: { description: "The applications this user can access and the roles they hold in each. Edit via the row menu → App access." },
      cell: ({ row }) => {
        const apps = rolesByUser?.get(row.original.id);
        if (!apps || apps.length === 0) {
          return <span className="text-muted-foreground">—</span>;
        }
        return (
          <span className="flex flex-col gap-1.5">
            {apps.map((app) => (
              <span key={app.project} className="flex flex-wrap items-center gap-1.5">
                <span className="text-muted-foreground text-xs">{app.project}</span>
                {app.roleKeys.map((k) => (
                  <span
                    key={k}
                    className={`inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium ${roleChipClass(k)}`}
                  >
                    {k}
                  </span>
                ))}
              </span>
            ))}
          </span>
        );
      },
    },
    {
      accessorKey: "state", header: "Status", filterFn: "equalsString",
      meta: { description: "Account lifecycle: Active (usable), Initial (awaiting first sign-in / password set), Locked, or Inactive (deactivated)." },
      cell: ({ row }) => {
        const s = row.original.state;
        return (
          <span className="inline-flex items-center gap-1.5 text-sm">
            <span aria-hidden
              className={`size-2 rounded-full ${STATE_DOT[s] ?? "bg-slate-400"}`} />
            {STATE_LABEL[s] ?? s}
          </span>
        );
      },
    },
    {
      id: "actions",
      cell: ({ row }) => {
        const u = row.original;
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
              {u.kind === "Human" && (
                <DropdownMenuItem data-testid="action-edit" onSelect={() => h.onEdit(u)}>
                  Edit profile
                </DropdownMenuItem>
              )}
              <DropdownMenuItem data-testid="action-grants"
                onSelect={() => h.onGrants(u)}>Access (grants)</DropdownMenuItem>
              <DropdownMenuItem data-testid="action-access"
                onSelect={() => h.onAccess(u)}>App access</DropdownMenuItem>
              {u.kind === "Machine" && (
                <DropdownMenuItem data-testid="action-keys"
                  onSelect={() => h.onKeys(u)}>Credentials (keys)</DropdownMenuItem>
              )}
              <DropdownMenuItem data-testid="action-deactivate"
                onSelect={() => h.onLifecycle(u, "deactivate")}>Deactivate</DropdownMenuItem>
              <DropdownMenuItem data-testid="action-reactivate"
                onSelect={() => h.onLifecycle(u, "reactivate")}>Reactivate</DropdownMenuItem>
              <DropdownMenuItem data-testid="action-lock"
                onSelect={() => h.onLifecycle(u, "lock")}>Lock</DropdownMenuItem>
              <DropdownMenuItem data-testid="action-unlock"
                onSelect={() => h.onLifecycle(u, "unlock")}>Unlock</DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem data-testid="action-delete"
                className="text-destructive" onSelect={() => h.onDelete(u)}>
                Delete (irreversible)
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        );
      },
    },
  ];
}
