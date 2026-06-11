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

export interface ColumnHandlers {
  onEdit: (u: User) => void;
  onDelete: (u: User) => void;
  onLifecycle: (u: User, action: Lifecycle) => void;
  onGrants: (u: User) => void;
}

// Status dot color per design language: ACTIVE=emerald, INITIAL=amber,
// LOCKED/INACTIVE=rose; anything else neutral.
const STATE_DOT: Partial<Record<UserState, string>> = {
  ACTIVE: "bg-emerald-500",
  INITIAL: "bg-amber-500",
  LOCKED: "bg-rose-500",
  INACTIVE: "bg-rose-500",
};

export function buildColumns(h: ColumnHandlers): ColumnDef<User>[] {
  return [
    {
      accessorKey: "userName", header: "Username",
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
            <span className="font-medium">{u.userName}</span>
          </span>
        );
      },
    },
    {
      accessorKey: "kind", header: "Type", filterFn: "equalsString",
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
      accessorKey: "state", header: "State", filterFn: "equalsString",
      cell: ({ row }) => {
        const s = row.original.state;
        return (
          <span className="inline-flex items-center gap-1.5 text-sm">
            <span aria-hidden
              className={`size-2 rounded-full ${STATE_DOT[s] ?? "bg-slate-400"}`} />
            {s}
          </span>
        );
      },
    },
    {
      accessorKey: "email", header: "Email",
      cell: ({ row }) =>
        row.original.email
          ? <span className="text-muted-foreground">{row.original.email}</span>
          : <span className="text-muted-foreground">—</span>,
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
