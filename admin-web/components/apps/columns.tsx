"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { AppWindow, MoreHorizontal } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { OidcApp } from "@/lib/types";

// Strip the verbose Zitadel enum prefix for display: OIDC_APP_TYPE_WEB → WEB,
// OIDC_AUTH_METHOD_TYPE_BASIC → BASIC, OIDC_GRANT_TYPE_AUTHORIZATION_CODE →
// AUTHORIZATION_CODE, APP_STATE_ACTIVE → ACTIVE. Unknown shapes pass through.
export const pretty = (s?: string) => (s ?? "").replace(/^[A-Z_]*?_(TYPE|STATE)_/, "");

// Tinted chip per OIDC app type (design language): NATIVE=emerald, WEB=blue,
// API=violet, USER_AGENT=amber; unknown=slate.
const APP_TYPE_CHIP: Record<string, string> = {
  NATIVE: "bg-emerald-500/10 text-emerald-700",
  WEB: "bg-blue-500/10 text-blue-700",
  API: "bg-violet-500/10 text-violet-700",
  USER_AGENT: "bg-amber-500/10 text-amber-700",
};

export interface AppColumnHandlers {
  onEdit: (a: OidcApp) => void;
  onRotate: (a: OidcApp) => void;
  onDelete: (a: OidcApp) => void;
}

export function buildAppColumns(h: AppColumnHandlers): ColumnDef<OidcApp>[] {
  return [
    {
      accessorKey: "name", header: "Name",
      cell: ({ row }) => (
        <span className="flex items-center gap-2.5">
          <span aria-hidden
            className="flex size-7 shrink-0 items-center justify-center rounded-md bg-violet-500/10 text-violet-600">
            <AppWindow className="size-4" />
          </span>
          <span className="font-medium">{row.original.name}</span>
        </span>
      ),
    },
    {
      accessorKey: "clientId", header: "Client ID",
      cell: ({ row }) => (
        <code className="font-mono text-xs text-muted-foreground">
          {row.original.oidcConfig?.clientId ?? "—"}
        </code>
      ),
    },
    {
      accessorKey: "appType", header: "Type",
      cell: ({ row }) => {
        const t = (row.original.oidcConfig?.appType ?? "").replace("OIDC_APP_TYPE_", "");
        if (!t) return <span className="text-muted-foreground">—</span>;
        return (
          <span
            className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${APP_TYPE_CHIP[t] ?? "bg-slate-500/10 text-slate-600"}`}
          >
            {t}
          </span>
        );
      },
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
