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
