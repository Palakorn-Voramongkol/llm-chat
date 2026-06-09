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
