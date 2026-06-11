"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import type { VisibilityState } from "@tanstack/react-table";
import { DataTable, TableColumnsToggle, TableDensityToggle, TableFilterToggle } from "@/components/ui/data-table";
import { useTableDensity } from "@/lib/use-table-density";
import { PageHeader } from "@/components/shell/PageHeader";
import { buildRoleColumns } from "@/components/roles/columns";
import { CreateRoleDialog } from "@/components/roles/create-role-dialog";
import { HoldersDialog } from "@/components/roles/holders-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { DetailPanel, PanelField, PanelSection } from "@/components/ui/detail-panel";
import { avatarGradient, initials } from "@/lib/avatar";
import { api, ApiError } from "@/lib/api";
import { useFilterOpen } from "@/lib/use-filter-open";
import type { Role, RoleHolder, RoleHolderList, RoleList } from "@/lib/types";

export default function RolesPage() {
  const [roles, setRoles] = useState<Role[]>([]);
  const [holdersByKey, setHoldersByKey] =
    useState<Map<string, RoleHolder[]>>(new Map());
  const [holdersTarget, setHoldersTarget] = useState<Role | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Role | null>(null);
  const [selected, setSelected] = useState<Role | null>(null);
  const [filterOpen, setFilterOpen] = useFilterOpen();
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
  const [density, setDensity] = useTableDensity();

  const load = useCallback(async () => {
    try {
      const list = await api.get<RoleList>("/api/roles");
      setRoles(list.result);
      // Holders per role for the avatar column — parallel + best-effort: a
      // failed lookup leaves that role's cell at "…", never blanks the page.
      const pairs = await Promise.all(
        (list.result ?? []).map(async (r): Promise<[string, RoleHolder[]] | null> => {
          try {
            const hl = await api.get<RoleHolderList>(
              `/api/roles/${encodeURIComponent(r.key)}/holders`,
            );
            return [r.key, hl.result ?? []];
          } catch {
            return null;
          }
        }),
      );
      setHoldersByKey(new Map(pairs.filter((p): p is [string, RoleHolder[]] => p !== null)));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load roles");
      }
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      // roleKey is a path param -> encode (design §7).
      await api.del(`/api/roles/${encodeURIComponent(deleteTarget.key)}`);
      toast.success("Role deleted");
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    } finally {
      setDeleteTarget(null);
      load();
    }
  }

  const columns = buildRoleColumns(
    {
      onHolders: setHoldersTarget,
      onDelete: setDeleteTarget,
    },
    holdersByKey,
  );

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6">
      <PageHeader
        title="Roles"
        description="Project roles and the users who hold them across the platform."
        actions={
          <>
            <TableFilterToggle open={filterOpen} onToggle={() => setFilterOpen((o) => !o)} />
            <TableColumnsToggle columns={columns} visibility={columnVisibility} onChange={setColumnVisibility} />
            <TableDensityToggle density={density} onChange={setDensity} />
            <CreateRoleDialog onCreated={load} />
          </>
        }
      />
      <div className="flex min-h-0 flex-1 gap-4">
        <div className="min-h-0 min-w-0 flex-1">
          <DataTable columns={columns} data={roles}
            filterFields={[
              { column: "key", label: "Key", placeholder: "Search key…" },
              { column: "displayName", label: "Display name", placeholder: "Search display name…" },
              { column: "group", label: "Group", placeholder: "Search group…" },
            ]}
            emptyMessage="No roles."
            getRowId={(r) => r.key}
            onRowClick={setSelected}
            selectedRowId={selected?.key ?? null}
            filterOpen={filterOpen}
            onFilterOpenChange={setFilterOpen}
            columnVisibility={columnVisibility}
            onColumnVisibilityChange={setColumnVisibility}
            density={density} />
        </div>
        <DetailPanel
          open={!!selected}
          title={selected?.key ?? ""}
          subtitle={selected?.displayName || undefined}
          onClose={() => setSelected(null)}
        >
          {selected && (() => {
            const holders = holdersByKey.get(selected.key);
            return (
              <>
                <PanelSection title="Role">
                  <PanelField label="Key" mono>{selected.key}</PanelField>
                  <PanelField label="Display">{selected.displayName || "—"}</PanelField>
                  <PanelField label="Group">{selected.group || "—"}</PanelField>
                </PanelSection>
                <PanelSection title={`Holders${holders ? ` (${holders.length})` : ""}`}>
                  {holders === undefined ? (
                    <p className="text-muted-foreground text-sm">Loading…</p>
                  ) : holders.length === 0 ? (
                    <p className="text-muted-foreground text-sm">No holders.</p>
                  ) : (
                    <ul className="space-y-2">
                      {holders.map((h) => {
                        const name = h.displayName || h.userName || h.userId;
                        return (
                          <li key={h.userId} className="flex items-center gap-2">
                            <span aria-hidden
                              className={`flex size-7 shrink-0 items-center justify-center rounded-full bg-linear-to-br text-[10px] font-bold text-white ${avatarGradient(h.userId)}`}>
                              {initials(name)}
                            </span>
                            <span className="min-w-0">
                              <span className="block truncate text-sm">{name}</span>
                              <span className="block truncate font-mono text-xs text-muted-foreground">
                                {h.userId}
                              </span>
                            </span>
                          </li>
                        );
                      })}
                    </ul>
                  )}
                </PanelSection>
              </>
            );
          })()}
        </DetailPanel>
      </div>
      <HoldersDialog role={holdersTarget} open={!!holdersTarget}
        onOpenChange={(o) => !o && setHoldersTarget(null)} />
      <ConfirmDialog open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete role?"
        description="This cascades — the role is stripped from every user grant that holds it. Deleting chat.admin can lock operators out of admin-web. This cannot be undone."
        confirmLabel="Delete role" onConfirm={confirmDelete} />
    </div>
  );
}
