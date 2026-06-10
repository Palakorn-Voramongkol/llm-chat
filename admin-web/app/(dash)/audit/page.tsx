"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { PageHeader } from "@/components/shell/PageHeader";
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
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6">
      <PageHeader
        title="Audit"
        description="Org-scoped event log from Zitadel."
      />

      {caps && !caps.events ? (
        <div
          role="alert"
          className="rounded-xl border border-amber-300 bg-amber-50 p-4 text-sm text-amber-900 shadow-sm dark:border-amber-900/50 dark:bg-amber-950/40 dark:text-amber-200"
        >
          <p className="font-medium">Audit unavailable</p>
          <p>Audit requires IAM_OWNER_VIEWER on the service account.</p>
          <p className="mt-1 text-amber-800/80 dark:text-amber-200/70">
            The least-privilege service account (ORG_USER_MANAGER +
            PROJECT_OWNER) cannot read the instance event log; granting the
            instance role is a separate, explicit decision (design §3/§11).
          </p>
        </div>
      ) : (
        <div className="flex-1 min-h-0">
          <DataTable
            columns={auditColumns}
            data={events}
            filterColumn="editor"
            filterPlaceholder="Filter by editor..."
            emptyMessage="No events."
          />
        </div>
      )}
    </div>
  );
}
