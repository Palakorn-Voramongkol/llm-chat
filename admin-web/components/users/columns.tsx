"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { MoreHorizontal } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { User } from "@/lib/types";

export type Lifecycle =
  | "deactivate" | "reactivate" | "lock" | "unlock" | "resend-init";

export interface ColumnHandlers {
  onEdit: (u: User) => void;
  onDelete: (u: User) => void;
  onLifecycle: (u: User, action: Lifecycle) => void;
}

export function buildColumns(h: ColumnHandlers): ColumnDef<User>[] {
  return [
    { accessorKey: "userName", header: "Username" },
    {
      accessorKey: "kind", header: "Type",
      cell: ({ row }) => <Badge variant="secondary">{row.original.kind}</Badge>,
    },
    {
      accessorKey: "state", header: "State",
      cell: ({ row }) => {
        const s = row.original.state;
        const variant = s === "ACTIVE" ? "default"
          : s === "INITIAL" ? "secondary" : "destructive";
        return <Badge variant={variant}>{s}</Badge>;
      },
    },
    { accessorKey: "email", header: "Email" },
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
