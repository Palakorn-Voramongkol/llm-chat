"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { MoreHorizontal, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { avatarGradient, initials } from "@/lib/avatar";
import type { Role, RoleHolder } from "@/lib/types";

export interface RoleColumnHandlers {
  onHolders: (r: Role) => void;
  onDelete: (r: Role) => void;
}

const MAX_AVATARS = 5;

/** Overlapping avatar stack of a role's holders; hovering an avatar names the
 * user. Clicking the stack opens the full holders dialog. */
function HolderAvatars({
  role, holders, onOpen,
}: {
  role: Role;
  holders: RoleHolder[] | undefined;
  onOpen: (r: Role) => void;
}) {
  if (!holders) return <span className="text-muted-foreground text-xs">…</span>;
  if (!holders.length) return <span className="text-muted-foreground">—</span>;
  const shown = holders.slice(0, MAX_AVATARS);
  const extra = holders.length - shown.length;
  return (
    <button
      type="button"
      onClick={() => onOpen(role)}
      className="flex items-center -space-x-2"
      aria-label={`${holders.length} holder${holders.length === 1 ? "" : "s"} of ${role.key}`}
    >
      {shown.map((holder) => {
        const name = holder.displayName || holder.userName || holder.userId;
        return (
          <span
            key={holder.userId}
            className={`ring-card flex size-7 items-center justify-center rounded-full bg-linear-to-br text-[10px] font-bold text-white ring-2 ${avatarGradient(holder.userId)}`}
          >
            {initials(name)}
          </span>
        );
      })}
      {extra > 0 && (
        <span className="ring-card bg-muted text-muted-foreground flex size-7 items-center justify-center rounded-full text-[10px] font-semibold ring-2">
          +{extra}
        </span>
      )}
    </button>
  );
}

export function buildRoleColumns(
  h: RoleColumnHandlers,
  holdersByKey?: Map<string, RoleHolder[]>,
): ColumnDef<Role>[] {
  return [
    {
      accessorKey: "key", header: "Key",
      cell: ({ row }) => (
        <span className="flex items-center gap-2.5">
          <span aria-hidden
            className="flex size-7 shrink-0 items-center justify-center rounded-md bg-indigo-500/10 text-indigo-600">
            <ShieldCheck className="size-4" />
          </span>
          <span className="font-mono text-sm font-medium">{row.original.key}</span>
        </span>
      ),
    },
    { accessorKey: "displayName", header: "Display name" },
    {
      accessorKey: "group", header: "Group",
      cell: ({ row }) =>
        row.original.group
          ? (
            <span className="inline-flex items-center rounded-full bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
              {row.original.group}
            </span>
          )
          : <span className="text-muted-foreground">—</span>,
    },
    {
      id: "holders",
      header: "Holders",
      enableSorting: false,
      cell: ({ row }) => (
        <HolderAvatars
          role={row.original}
          holders={holdersByKey?.get(row.original.key)}
          onOpen={h.onHolders}
        />
      ),
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
