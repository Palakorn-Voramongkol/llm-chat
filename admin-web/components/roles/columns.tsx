"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { MoreHorizontal, ShieldCheck } from "lucide-react";
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
