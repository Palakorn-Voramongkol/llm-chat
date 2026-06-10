"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { Badge } from "@/components/ui/badge";
import type { AuditEvent } from "@/lib/types";

export const auditColumns: ColumnDef<AuditEvent>[] = [
  {
    id: "editor",
    accessorFn: (e) => e.editor?.displayName ?? e.editor?.userId ?? "—",
    header: "Editor",
  },
  {
    id: "eventType",
    accessorFn: (e) => e.type?.localized?.localizedMessage ?? e.type?.type ?? "—",
    header: "Event",
    cell: ({ getValue }) => <Badge variant="secondary">{getValue<string>()}</Badge>,
  },
  {
    id: "aggregate",
    accessorFn: (e) =>
      e.aggregate?.type ? `${e.aggregate.type}/${e.aggregate.id ?? ""}` : (e.aggregate?.id ?? "—"),
    header: "Aggregate",
  },
  {
    id: "creationDate",
    accessorFn: (e) => e.creationDate ?? "",
    header: "Date",
    cell: ({ getValue }) => {
      const raw = getValue<string>();
      return raw ? new Date(raw).toLocaleString() : "—";
    },
  },
];
