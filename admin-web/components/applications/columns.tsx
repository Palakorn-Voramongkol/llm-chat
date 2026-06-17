"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { Boxes } from "lucide-react";
import type { AppProject } from "@/lib/types";

// Per-application aggregates computed on the page (best-effort): the app's role
// keys and the count of distinct grant holders. Keyed by project id.
export interface AppMeta {
  roleKeys: string[]; // the app's role keys (for the chips)
  userCount: number; // distinct users with a grant on the app
}

export function buildApplicationColumns(
  metaById?: Map<string, AppMeta>,
): ColumnDef<AppProject>[] {
  return [
    {
      accessorKey: "name", header: "Application",
      cell: ({ row }) => {
        const p = row.original;
        return (
          <span className="flex items-center gap-2.5">
            <span aria-hidden
              className="flex size-8 shrink-0 items-center justify-center rounded-md bg-violet-500/10 text-violet-600">
              <Boxes className="size-4" />
            </span>
            <span className="flex min-w-0 flex-col">
              <span className="truncate font-medium">{p.name || p.id}</span>
              <span className="truncate font-mono text-xs text-muted-foreground">
                {p.id}
              </span>
            </span>
          </span>
        );
      },
    },
    {
      id: "roles", header: "Roles", enableSorting: false,
      cell: ({ row }) => {
        const meta = metaById?.get(row.original.id);
        if (!meta) return <span className="text-muted-foreground text-xs">…</span>;
        if (meta.roleKeys.length === 0) {
          return <span className="text-muted-foreground">—</span>;
        }
        return (
          <span className="flex flex-wrap items-center gap-1.5">
            <span className="text-muted-foreground text-xs tabular-nums">
              {meta.roleKeys.length}
            </span>
            {meta.roleKeys.slice(0, 6).map((k) => (
              <span key={k}
                className="inline-flex items-center rounded-md bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
                {k}
              </span>
            ))}
            {meta.roleKeys.length > 6 && (
              <span className="text-muted-foreground text-xs">
                +{meta.roleKeys.length - 6}
              </span>
            )}
          </span>
        );
      },
    },
    {
      id: "users", header: "Users", enableSorting: false,
      cell: ({ row }) => {
        const meta = metaById?.get(row.original.id);
        if (!meta) return <span className="text-muted-foreground text-xs">…</span>;
        return (
          <span className="tabular-nums text-sm">{meta.userCount}</span>
        );
      },
    },
  ];
}
