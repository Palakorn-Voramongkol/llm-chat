"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import type { VisibilityState } from "@tanstack/react-table";
import { DataTable, TableColumnsToggle, TableFilterToggle } from "@/components/ui/data-table";
import { PageHeader } from "@/components/shell/PageHeader";
import { auditColumns } from "@/components/audit/columns";
import { DetailPanel, PanelField, PanelSection } from "@/components/ui/detail-panel";
import { eventChipClass, eventLabel } from "@/lib/event-style";
import { api, ApiError } from "@/lib/api";
import { useFilterOpen } from "@/lib/use-filter-open";
import type { AuditEvent, Capabilities, EventList } from "@/lib/types";

export default function AuditPage() {
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [selected, setSelected] = useState<AuditEvent | null>(null);
  const [filterOpen, setFilterOpen] = useFilterOpen();
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});

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
        actions={
          caps?.events ? (
            <>
              <TableFilterToggle open={filterOpen} onToggle={() => setFilterOpen((o) => !o)} />
              <TableColumnsToggle columns={auditColumns} visibility={columnVisibility} onChange={setColumnVisibility} />
            </>
          ) : undefined
        }
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
        <div className="flex min-h-0 flex-1 gap-4">
          <div className="min-h-0 flex-1">
            <DataTable
              columns={auditColumns}
              data={events}
              filterFields={[
                { column: "editor", label: "Editor", placeholder: "Search editor…" },
                { column: "eventType", label: "Event", placeholder: "Search event…" },
              ]}
              emptyMessage="No events."
              getRowId={(e) => e.sequence ?? ""}
              onRowClick={setSelected}
              selectedRowId={selected?.sequence ?? null}
              filterOpen={filterOpen}
              onFilterOpenChange={setFilterOpen}
              columnVisibility={columnVisibility}
              onColumnVisibilityChange={setColumnVisibility}
            />
          </div>
          <DetailPanel
            open={!!selected}
            title={selected ? eventLabel(selected.type) : ""}
            subtitle={selected?.sequence ? `seq ${selected.sequence}` : undefined}
            onClose={() => setSelected(null)}
          >
            {selected && (
              <>
                <PanelSection title="Event">
                  <div className="mb-1">
                    <span className={eventChipClass(selected.type?.type)}>
                      {eventLabel(selected.type)}
                    </span>
                  </div>
                  <PanelField label="Raw type" mono>{selected.type?.type || "—"}</PanelField>
                  <PanelField label="Sequence" mono>{selected.sequence || "—"}</PanelField>
                  <PanelField label="When">
                    {selected.creationDate
                      ? new Date(selected.creationDate).toLocaleString()
                      : "—"}
                  </PanelField>
                </PanelSection>
                <PanelSection title="Editor">
                  <PanelField label="Name">
                    {selected.editor?.displayName || selected.editor?.service || "—"}
                  </PanelField>
                  <PanelField label="User ID" mono>{selected.editor?.userId || "—"}</PanelField>
                  <PanelField label="Service">{selected.editor?.service || "—"}</PanelField>
                </PanelSection>
                <PanelSection title="Aggregate">
                  <PanelField label="Type">
                    {selected.aggregate?.type?.localized?.localizedMessage
                      || selected.aggregate?.type?.type
                      || "—"}
                  </PanelField>
                  <PanelField label="ID" mono>{selected.aggregate?.id || "—"}</PanelField>
                  <PanelField label="Resource owner" mono>
                    {selected.aggregate?.resourceOwner || "—"}
                  </PanelField>
                </PanelSection>
              </>
            )}
          </DetailPanel>
        </div>
      )}
    </div>
  );
}
