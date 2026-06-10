"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { auditColumns } from "@/components/audit/columns";
import { api, ApiError } from "@/lib/api";
import type { AuditEvent, Capabilities, EventList } from "@/lib/types";

export default function AuditPage() {
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [events, setEvents] = useState<AuditEvent[]>([]);

  const load = useCallback(async () => {
    try {
      const c = await api.get<Capabilities>("/api/capabilities");
      setCaps(c);
      // FAIL CLOSED: only read the event log when the capability is present (§11).
      if (!c.events) return;
      const list = await api.get<EventList>("/api/events");
      setEvents(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error(e instanceof ApiError ? e.message : "Failed to load audit log");
      }
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div className="space-y-4 px-6 py-6">
      <div>
        <h1 className="text-xl font-bold">Audit</h1>
        <p className="text-muted-foreground text-sm">
          Org-scoped event log from Zitadel.
        </p>
      </div>

      {caps && !caps.events ? (
        <div
          role="alert"
          className="rounded-md border border-amber-300 bg-amber-50 p-4 text-sm text-amber-900 dark:border-amber-900/50 dark:bg-amber-950/40 dark:text-amber-200"
        >
          <p className="font-medium">Audit unavailable</p>
          <p>Audit requires IAM_OWNER_VIEWER on the service account.</p>
          <p className="mt-1 text-amber-800/80 dark:text-amber-200/70">
            ORG_OWNER cannot read the instance event log; granting the instance
            role is a separate, explicit decision (design §3/§11).
          </p>
        </div>
      ) : (
        <DataTable
          columns={auditColumns}
          data={events}
          filterColumn="editor"
          filterPlaceholder="Filter by editor..."
          emptyMessage="No events."
        />
      )}
    </div>
  );
}
