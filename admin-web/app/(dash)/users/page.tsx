"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import type { GroupingState, VisibilityState } from "@tanstack/react-table";
import { DataTable, TableColumnsToggle, TableDensityToggle, TableFilterToggle, TableGroupToggle } from "@/components/ui/data-table";
import { useTableDensity } from "@/lib/use-table-density";
import { PageHeader } from "@/components/shell/PageHeader";
import { buildColumns, type Lifecycle } from "@/components/users/columns";
import { CreateUserDialog } from "@/components/users/create-user-dialog";
import { EditUserDialog } from "@/components/users/edit-user-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { GrantsDialog } from "@/components/users/grants-dialog";
import { DetailPanel, PanelField, PanelSection } from "@/components/ui/detail-panel";
import { Badge } from "@/components/ui/badge";
import { api, ApiError } from "@/lib/api";
import { useFilterOpen } from "@/lib/use-filter-open";
import type {
  ChatClient, ChatSessions, GrantList, SigninList, User, UserList,
} from "@/lib/types";

export default function UsersPage() {
  const [users, setUsers] = useState<User[]>([]);
  const [editTarget, setEditTarget] = useState<User | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);
  const [grantsTarget, setGrantsTarget] = useState<User | null>(null);
  const [selected, setSelected] = useState<User | null>(null);
  const [filterOpen, setFilterOpen] = useFilterOpen();
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
  const [density, setDensity] = useTableDensity();
  const [grouping, setGrouping] = useState<GroupingState>([]);
  const [grantsFor, setGrantsFor] =
    useState<{ id: string; list: GrantList } | null>(null);
  // Per-user monitoring joins: last sign-in time + live chat sessions.
  const [lastSignIn, setLastSignIn] = useState<Map<string, string>>(new Map());
  const [liveByUser, setLiveByUser] = useState<Map<string, ChatClient[]>>(new Map());

  // Fetch the selected user's grants for the side panel. Keyed by user id so
  // the panel shows "Loading…" until the CURRENT selection's fetch resolves and
  // never flashes the previous user's grants. setState lives only in the async
  // callback (no synchronous setState in the effect body).
  useEffect(() => {
    if (!selected) return;
    let alive = true;
    const id = selected.id;
    api
      .get<GrantList>(`/api/users/${id}/grants`)
      .then((g) => alive && setGrantsFor({ id, list: g }))
      .catch(() => alive && setGrantsFor({ id, list: { result: [] } }));
    return () => {
      alive = false;
    };
  }, [selected]);

  const selGrants =
    selected && grantsFor?.id === selected.id ? grantsFor.list : null;

  const load = useCallback(async () => {
    try {
      const list = await api.get<UserList>("/api/users");
      setUsers(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load users");
      }
    }
    // Best-effort monitoring joins (each degrades on its own; never blocks the
    // user list). Last sign-in per user from the audit sign-in feed; live chat
    // sessions per user from the manager.
    try {
      const s = await api.get<SigninList>("/api/signins");
      const m = new Map<string, string>();
      if (s.available) {
        for (const e of s.result ?? []) {
          const uid = e.editor?.userId;
          if (uid && e.creationDate && !m.has(uid)) m.set(uid, e.creationDate);
        }
      }
      setLastSignIn(m);
    } catch {
      setLastSignIn(new Map());
    }
    try {
      const c = await api.get<ChatSessions>("/api/chat-sessions");
      const m = new Map<string, ChatClient[]>();
      for (const client of c.clients?.clients ?? []) {
        const arr = m.get(client.userId) ?? [];
        arr.push(client);
        m.set(client.userId, arr);
      }
      setLiveByUser(m);
    } catch {
      setLiveByUser(new Map());
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  async function onLifecycle(u: User, action: Lifecycle) {
    try {
      await api.post(`/api/users/${u.id}/${action}`);
      toast.success(`${action} ok`);
      load();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : `${action} failed`);
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      await api.del(`/api/users/${deleteTarget.id}`);
      toast.success("User deleted");
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    } finally {
      setDeleteTarget(null);
      load();
    }
  }

  const columns = buildColumns({
    onEdit: setEditTarget,
    onDelete: setDeleteTarget,
    onLifecycle,
    onGrants: setGrantsTarget,
  });

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6">
      <PageHeader
        title="Users"
        description="People and machine accounts across every app on the platform."
        actions={
          <>
            <TableFilterToggle open={filterOpen} onToggle={() => setFilterOpen((o) => !o)} />
            <TableGroupToggle
              groupable={[{ column: "kind", label: "Type" }, { column: "state", label: "State" }]}
              grouping={grouping} onChange={setGrouping} />
            <TableColumnsToggle columns={columns} visibility={columnVisibility} onChange={setColumnVisibility} />
            <TableDensityToggle density={density} onChange={setDensity} />
            <CreateUserDialog onCreated={load} />
          </>
        }
      />
      <div className="flex min-h-0 flex-1 gap-4">
        <div className="min-h-0 min-w-0 flex-1">
          <DataTable columns={columns} data={users}
            filterFields={[
              { column: "userName", label: "Username", placeholder: "Search username…" },
              { column: "email", label: "Email", placeholder: "Search email…" },
              { column: "kind", label: "Type", options: [
                { value: "Human", label: "Human" },
                { value: "Machine", label: "Machine" },
              ] },
              { column: "state", label: "State", options: [
                { value: "ACTIVE", label: "Active" },
                { value: "INACTIVE", label: "Inactive" },
                { value: "LOCKED", label: "Locked" },
                { value: "INITIAL", label: "Initial" },
              ] },
            ]}
            emptyMessage="No users."
            getRowId={(u) => u.id}
            onRowClick={setSelected}
            selectedRowId={selected?.id ?? null}
            filterOpen={filterOpen}
            onFilterOpenChange={setFilterOpen}
            columnVisibility={columnVisibility}
            onColumnVisibilityChange={setColumnVisibility}
            density={density}
            grouping={grouping}
            onGroupingChange={setGrouping} />
        </div>
        <DetailPanel
          open={!!selected}
          title={selected?.userName ?? ""}
          subtitle={selected?.kind}
          onClose={() => setSelected(null)}
        >
          {selected && (
            <>
              <PanelSection title="Identity">
                <PanelField label="ID" mono>{selected.id || "—"}</PanelField>
                <PanelField label="Username">{selected.userName}</PanelField>
                <PanelField label="Display">{selected.displayName || "—"}</PanelField>
                <PanelField label="Email">{selected.email || "—"}</PanelField>
                <PanelField label="Type">{selected.kind}</PanelField>
                <PanelField label="State">{selected.state}</PanelField>
              </PanelSection>
              <PanelSection title="Activity">
                {(() => {
                  const last = lastSignIn.get(selected.id);
                  const live = liveByUser.get(selected.id) ?? [];
                  const totalQ = live.reduce((n, c) => n + (c.questionsSent ?? 0), 0);
                  return (
                    <>
                      <PanelField label="Last sign-in">
                        {last ? new Date(last).toLocaleString() : "—"}
                      </PanelField>
                      <PanelField label="Live sessions">
                        {live.length ? (
                          <Badge variant={live.length ? "default" : "secondary"}>
                            {live.length} active
                          </Badge>
                        ) : "—"}
                      </PanelField>
                      {live.length > 0 && (
                        <>
                          <PanelField label="Questions">{totalQ}</PanelField>
                          <PanelField label="Last active">
                            {(() => {
                              const t = live
                                .map((c) => c.lastQAt)
                                .filter(Boolean)
                                .sort()
                                .at(-1);
                              return t ? new Date(t).toLocaleString() : "—";
                            })()}
                          </PanelField>
                        </>
                      )}
                    </>
                  );
                })()}
              </PanelSection>
              <PanelSection title="Access (grants)">
                {selGrants === null ? (
                  <p className="text-muted-foreground text-sm">Loading…</p>
                ) : selGrants.result.length === 0 ? (
                  <p className="text-muted-foreground text-sm">No grants.</p>
                ) : (
                  <ul className="space-y-2">
                    {selGrants.result.map((g, i) => (
                      <li key={g.grantId || g.projectId || i}>
                        <div className="font-mono text-xs text-muted-foreground">
                          {g.projectId}
                        </div>
                        <div className="mt-1 flex flex-wrap gap-1">
                          {g.roleKeys.length ? (
                            g.roleKeys.map((rk) => (
                              <Badge key={rk} variant="secondary">{rk}</Badge>
                            ))
                          ) : (
                            <span className="text-muted-foreground text-xs">—</span>
                          )}
                        </div>
                      </li>
                    ))}
                  </ul>
                )}
              </PanelSection>
            </>
          )}
        </DetailPanel>
      </div>
      <EditUserDialog user={editTarget} open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)} onSaved={load} />
      <ConfirmDialog open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete user?"
        description="This is irreversible and removes the user and any machine keys. Already-issued tokens stay valid until their TTL expires."
        confirmLabel="Delete" onConfirm={confirmDelete} />
      <GrantsDialog user={grantsTarget} open={!!grantsTarget}
        onOpenChange={(o) => !o && setGrantsTarget(null)} onSaved={load} />
    </div>
  );
}
