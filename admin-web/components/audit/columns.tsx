"use client";
import type { ColumnDef } from "@tanstack/react-table";
import { initials } from "@/lib/avatar";
import { eventChipClass, eventLabel } from "@/lib/event-style";
import type { AuditEvent } from "@/lib/types";

// Cheap relative hint ("3m ago"); empty for future/invalid dates.
function relativeHint(raw: string): string {
  const t = new Date(raw).getTime();
  if (Number.isNaN(t)) return "";
  const s = Math.floor((Date.now() - t) / 1000);
  if (s < 0) return "";
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

export const auditColumns: ColumnDef<AuditEvent>[] = [
  {
    id: "editor",
    accessorFn: (e) =>
      e.editor?.displayName ?? e.editor?.userId ?? e.editor?.service ?? "—",
    header: "Editor",
    cell: ({ row, getValue }) => {
      const name = getValue<string>();
      const isService = !row.original.editor?.displayName
        && !row.original.editor?.userId
        && !!row.original.editor?.service;
      return (
        <span className="flex items-center gap-2">
          <span aria-hidden
            className="flex size-6 shrink-0 items-center justify-center rounded-full bg-slate-500/10 text-[10px] font-bold text-slate-600">
            {isService ? "Z" : initials(name)}
          </span>
          <span className="text-sm">{name}</span>
        </span>
      );
    },
  },
  {
    id: "eventType",
    accessorFn: (e) => eventLabel(e.type),
    header: "Event",
    cell: ({ row, getValue }) => (
      <span className={eventChipClass(row.original.type?.type)}>
        {getValue<string>()}
      </span>
    ),
  },
  {
    id: "aggregate",
    accessorFn: (e) => {
      // aggregate.type is a localized enum object, not a string.
      const label =
        e.aggregate?.type?.localized?.localizedMessage ?? e.aggregate?.type?.type;
      const id = e.aggregate?.id ?? "";
      return label ? `${label}${id ? ` · ${id}` : ""}` : id || "—";
    },
    header: "Aggregate",
    cell: ({ row }) => {
      const e = row.original;
      const label =
        e.aggregate?.type?.localized?.localizedMessage ?? e.aggregate?.type?.type;
      const id = e.aggregate?.id ?? "";
      if (!label && !id) return <span className="text-muted-foreground">—</span>;
      return (
        <span className="flex items-baseline gap-1.5">
          {label && <span className="font-medium">{label}</span>}
          {id && <span className="font-mono text-xs text-muted-foreground">{id}</span>}
        </span>
      );
    },
  },
  {
    id: "creationDate",
    accessorFn: (e) => e.creationDate ?? "",
    header: "Date",
    cell: ({ getValue }) => {
      const raw = getValue<string>();
      if (!raw) return <span className="text-muted-foreground">—</span>;
      const hint = relativeHint(raw);
      return (
        <span className="flex items-baseline gap-1.5">
          <span>{new Date(raw).toLocaleString()}</span>
          {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
        </span>
      );
    },
  },
];
