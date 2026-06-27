"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import type { GroupingState, VisibilityState } from "@tanstack/react-table";
import { DataTable, TableColumnsToggle, TableDensityToggle, TableFilterToggle, TableGroupToggle } from "@/components/ui/data-table";
import { useTableDensity } from "@/lib/use-table-density";
import { PageHeader } from "@/components/shell/PageHeader";
import { Boxes } from "lucide-react";
import { buildColumns, roleChipFull, fmtCount, fmtBytes, type Lifecycle, type AppAccess } from "@/components/users/columns";
import { CreateUserDialog } from "@/components/users/create-user-dialog";
import { EditUserDialog } from "@/components/users/edit-user-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { GrantsDialog } from "@/components/users/grants-dialog";
import { AccessDialog } from "@/components/users/access-dialog";
import { KeysDialog } from "@/components/users/keys-dialog";
import { UsageTrend } from "@/components/users/usage-trend";
import { DetailPanel, PanelField, PanelSection } from "@/components/ui/detail-panel";
import { SandboxTree } from "@/components/users/sandbox-tree";
import { buildTree } from "@/lib/sandbox-tree";
import { Badge } from "@/components/ui/badge";
import { api, ApiError } from "@/lib/api";
import { useFilterOpen } from "@/lib/use-filter-open";
import type {
  AppProjectList, ChatClient, ChatSessions, GrantList,
  SigninList, User, UserList, UsageRow, UsageResponse, DailyRow, UsageDailyResponse,
  SandboxFiles,
} from "@/lib/types";

export default function UsersPage() {
  const [users, setUsers] = useState<User[]>([]);
  // userId -> the user's access grouped per application ({ app name, role
  // keys }). A user absent from the map had its grants fetch fail (cell shows
  // "—"). Grouping is preserved (not flattened) so the column can show which
  // roles apply to which app.
  const [rolesByUser, setRolesByUser] = useState<Map<string, AppAccess[]>>(new Map());
  // projectId -> application name, for labelling a user's grants by app.
  const [projectNames, setProjectNames] = useState<Map<string, string>>(new Map());
  const [editTarget, setEditTarget] = useState<User | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);
  const [grantsTarget, setGrantsTarget] = useState<User | null>(null);
  const [accessTarget, setAccessTarget] = useState<User | null>(null);
  const [keysTarget, setKeysTarget] = useState<User | null>(null);
  const [selected, setSelected] = useState<User | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [filterOpen, setFilterOpen] = useFilterOpen();
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({});
  const [density, setDensity] = useTableDensity();
  const [grouping, setGrouping] = useState<GroupingState>([]);
  const [grantsFor, setGrantsFor] =
    useState<{ id: string; list: GrantList } | null>(null);
  // Per-user token-usage stats keyed by userId. A failed /api/usage fetch
  // leaves this map empty (columns show "—"), never blanks the page.
  const [usageByUser, setUsageByUser] = useState<Map<string, UsageRow>>(new Map());
  // Per-user daily token buckets (last 30 days) for the detail-panel trend.
  const [dailyByUser, setDailyByUser] = useState<Map<string, DailyRow[]>>(new Map());
  // Per-user monitoring joins: last sign-in time + live chat sessions.
  const [lastSignIn, setLastSignIn] = useState<Map<string, string>>(new Map());
  const [liveByUser, setLiveByUser] = useState<Map<string, ChatClient[]>>(new Map());
  // The selected user's confined claude sandbox tree (read-only). null = loading.
  const [sandbox, setSandbox] = useState<SandboxFiles | null>(null);

  // Fetch the selected user's confined sandbox tree for the side panel.
  // Best-effort: a sandbox error degrades only this section, never the panel.
  useEffect(() => {
    if (!selected) { setSandbox(null); return; }
    let alive = true;
    setSandbox(null); // loading
    api
      .get<SandboxFiles>(`/api/users/${selected.id}/files`)
      .then((s) => { if (alive) setSandbox(s); })
      .catch(() => { if (alive) setSandbox({ configured: true, ok: false, error: "Failed to load sandbox" }); });
    return () => { alive = false; };
  }, [selected]);

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
    let loaded: User[] = [];
    try {
      const list = await api.get<UserList>("/api/users");
      loaded = list.result ?? [];
      setUsers(loaded);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load users");
      }
    }
    // Resolve projectId -> application name (best-effort): used to label each
    // app group in the grants column. An unknown id falls back to the raw id.
    const nameByProject = new Map<string, string>();
    try {
      const pl = await api.get<AppProjectList>("/api/projects");
      for (const p of pl.result ?? []) nameByProject.set(p.id, p.name || p.id);
      setProjectNames(nameByProject);
    } catch {
      /* leave empty: ids will be shown raw as the group label */
    }
    // Per-user grants in parallel + best-effort (like roles/page.tsx holders): a
    // failed lookup simply omits that user so its access cell shows "—". Never
    // blocks the list. Group roleKeys by projectId and map to the app name —
    // grouping is preserved (not flattened).
    try {
      const pairs = await Promise.all(
        loaded.map(async (u): Promise<[string, AppAccess[]] | null> => {
          try {
            const g = await api.get<GrantList>(`/api/users/${u.id}/grants`);
            // Merge any multiple grants on the same project, dedupe keys.
            const keysByProject = new Map<string, Set<string>>();
            for (const gr of g.result ?? []) {
              const set = keysByProject.get(gr.projectId) ?? new Set<string>();
              for (const k of gr.roleKeys) set.add(k);
              keysByProject.set(gr.projectId, set);
            }
            const access: AppAccess[] = [...keysByProject.entries()].map(
              ([pid, keys]) => ({
                project: nameByProject.get(pid) ?? pid,
                roleKeys: [...keys],
              }),
            );
            return [u.id, access];
          } catch {
            return null;
          }
        }),
      );
      setRolesByUser(
        new Map(pairs.filter((p): p is [string, AppAccess[]] => p !== null)),
      );
    } catch {
      setRolesByUser(new Map());
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
    // Per-user token-usage stats: best-effort, same pattern as holdersByKey.
    // A failed fetch leaves the map empty; columns show "—", page never blanks.
    try {
      const u = await api.get<UsageResponse>("/api/usage");
      const m = new Map<string, UsageRow>();
      for (const row of u.users ?? []) {
        if (row.userId) m.set(row.userId, row);
      }
      setUsageByUser(m);
    } catch {
      setUsageByUser(new Map());
    }
    // Per-user daily buckets (last 30 days): best-effort, same pattern. A failed
    // fetch leaves the map empty; the trend shows its empty state.
    try {
      const r = await api.get<UsageDailyResponse>("/api/usage-daily");
      const m = new Map<string, DailyRow[]>();
      for (const row of r.days ?? []) {
        if (!row.userId) continue;
        const list = m.get(row.userId) ?? [];
        list.push(row);
        m.set(row.userId, list);
      }
      setDailyByUser(m);
    } catch {
      setDailyByUser(new Map());
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

  const columns = buildColumns(
    {
      onEdit: setEditTarget,
      onDelete: setDeleteTarget,
      onLifecycle,
      onGrants: setGrantsTarget,
      onAccess: setAccessTarget,
      onKeys: setKeysTarget,
    },
    rolesByUser,
    usageByUser,
  );


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
            <CreateUserDialog onCreated={load} open={createOpen} onOpenChange={setCreateOpen} />
          </>
        }
      />
      <div className="flex min-h-0 flex-1 gap-4">
        <div className="flex min-w-0 flex-1 flex-col gap-4">
          <div className="min-h-0 flex-1">
          <DataTable columns={columns} data={users}
            filterFields={[
              { column: "userName", label: "Username", placeholder: "Search username…" },
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
              {/* Self-counted usage breakdown — the platform's OWN per-user
                  counts from /api/usage (request/answer text + attachment bytes),
                  not claude's shared account-level tokens. If the fetch failed,
                  usageByUser is empty and every field shows "—". */}
              <PanelSection title="Usage">
                {(() => {
                  const u = usageByUser.get(selected.id);
                  return (
                    <>
                      <PanelField label="Requests">{u ? fmtCount(u.requests) : "—"}</PanelField>
                      <PanelField label="Chars in">{u ? fmtCount(u.charsIn) : "—"}</PanelField>
                      <PanelField label="Files">{u ? fmtCount(u.files) : "—"}</PanelField>
                      <PanelField label="File bytes">{u ? fmtBytes(u.fileBytes) : "—"}</PanelField>
                      <PanelField label="Chars out">{u ? fmtCount(u.charsOut) : "—"}</PanelField>
                      <PanelField label="Last used">{u?.lastUsed ? new Date(u.lastUsed).toLocaleString() : "—"}</PanelField>
                    </>
                  );
                })()}
                <div className="pt-3">
                  <UsageTrend rows={dailyByUser.get(selected.id)} endDate={new Date()} />
                </div>
              </PanelSection>
              {/* App access & roles — what applications this user can use and
                  the roles they hold in each (read view; edit via "App access"). */}
              <PanelSection title="App access & roles">
                {selGrants === null ? (
                  <p className="text-muted-foreground text-sm">Loading…</p>
                ) : selGrants.result.length === 0 ? (
                  <p className="text-muted-foreground text-sm">
                    No application access. Use “App access” to grant some.
                  </p>
                ) : (
                  <ul className="space-y-2.5">
                    {selGrants.result.map((g, i) => (
                      <li key={g.grantId || g.projectId || i}
                        className="rounded-lg border p-2.5">
                        <div className="flex items-center gap-2">
                          <span aria-hidden
                            className="flex size-6 shrink-0 items-center justify-center rounded-md bg-violet-500/10 text-violet-600">
                            <Boxes className="size-3.5" />
                          </span>
                          <span className="text-sm font-medium">
                            {projectNames.get(g.projectId) ?? g.projectId}
                          </span>
                        </div>
                        <div className="mt-1.5 flex flex-wrap gap-1 pl-8">
                          {g.roleKeys.length ? (
                            g.roleKeys.map((rk) => (
                              <span key={rk} className={roleChipFull(rk)}>{rk}</span>
                            ))
                          ) : (
                            <span className="text-muted-foreground text-xs">no roles</span>
                          )}
                        </div>
                      </li>
                    ))}
                  </ul>
                )}
              </PanelSection>
              <PanelSection title="Sandbox">
                {!sandbox ? (
                  <span className="text-muted-foreground text-sm">Loading…</span>
                ) : sandbox.configured === false ? (
                  <span className="text-muted-foreground text-sm">Sandbox view not configured (MANAGER_CONTROL_URL).</span>
                ) : sandbox.ok === false ? (
                  <span className="text-destructive text-sm">{sandbox.error || "Sandbox unavailable"}</span>
                ) : (sandbox.entries ?? []).length === 0 ? (
                  <span className="text-muted-foreground text-sm">No sandbox yet.</span>
                ) : (
                  <>
                    <SandboxTree nodes={buildTree(sandbox.entries ?? [])} />
                    {sandbox.truncated && (
                      <p className="text-muted-foreground mt-2 text-xs">Showing first 2000 entries (truncated).</p>
                    )}
                  </>
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
      <AccessDialog user={accessTarget} open={!!accessTarget}
        onOpenChange={(o) => !o && setAccessTarget(null)} onSaved={load} />
      <KeysDialog user={keysTarget} open={!!keysTarget}
        onOpenChange={(o) => !o && setKeysTarget(null)} />
    </div>
  );
}
