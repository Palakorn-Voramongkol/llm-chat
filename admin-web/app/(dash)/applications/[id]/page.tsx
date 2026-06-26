"use client";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { ArrowLeft, ShieldCheck, AppWindow, Pencil, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/shell/PageHeader";
import { DetailPanel, PanelField, PanelSection } from "@/components/ui/detail-panel";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { RoleCreateDialog } from "@/components/applications/role-create-dialog";
import { RoleEditDialog } from "@/components/roles/role-edit-dialog";
import { AppFormDialog } from "@/components/apps/app-form-dialog";
import { SecretRevealDialog } from "@/components/apps/secret-reveal-dialog";
import { APP_TYPE_CHIP, appTypeLabel, pretty } from "@/components/apps/columns";
import { avatarGradient, initials } from "@/lib/avatar";
import { api, ApiError } from "@/lib/api";
import { clientPath, clientSecretPath } from "@/lib/clients";
import type {
  AppProject, AppProjectList, OidcApp, OidcAppList,
  ProjectGrant, ProjectGrantList, Role, RoleList, AppSecret,
} from "@/lib/types";

export default function ApplicationDetailPage() {
  const params = useParams<{ id: string }>();
  const id = params.id;
  const [project, setProject] = useState<AppProject | null>(null);
  const [roles, setRoles] = useState<Role[]>([]);
  const [clients, setClients] = useState<OidcApp[]>([]);
  const [grants, setGrants] = useState<ProjectGrant[]>([]);
  const [selected, setSelected] = useState<OidcApp | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<OidcApp | null>(null);
  const [rotateTarget, setRotateTarget] = useState<OidcApp | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<OidcApp | null>(null);
  const [revealed, setRevealed] = useState<AppSecret | null>(null);
  const [deleteRole, setDeleteRole] = useState<Role | null>(null);
  const [editRole, setEditRole] = useState<Role | null>(null);

  const loadRoles = useCallback(async () => {
    if (!id) return;
    try {
      const rl = await api.get<RoleList>(`/api/projects/${id}/roles`);
      setRoles(rl.result ?? []);
    } catch { setRoles([]); }
  }, [id]);

  const loadClients = useCallback(async () => {
    if (!id) return;
    try {
      const al = await api.get<OidcAppList>(`/api/projects/${id}/apps`);
      setClients(al.result ?? []);
    } catch { /* keep the prior list on a refresh failure (e.g. after a mutation) */ }
  }, [id]);

  const load = useCallback(async () => {
    if (!id) return;
    try {
      const list = await api.get<AppProjectList>("/api/projects");
      setProject((list.result ?? []).find((p) => p.id === id) ?? null);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load application");
      }
    }
    await Promise.all([
      loadRoles(),
      loadClients(),
      (async () => {
        try {
          const gl = await api.get<ProjectGrantList>(`/api/projects/${id}/grants`);
          setGrants(gl.result ?? []);
        } catch { setGrants([]); }
      })(),
    ]);
  }, [id, loadRoles, loadClients]);

  useEffect(() => { load(); }, [load]);

  async function confirmRotate() {
    if (!rotateTarget) return;
    const target = rotateTarget;
    setRotateTarget(null);
    try {
      const s = await api.post<AppSecret>(clientSecretPath(id, target.id));
      toast.success("Secret rotated");
      if (s?.clientSecret) setRevealed(s); // one-time reveal
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Rotate failed");
    }
  }

  async function confirmDeleteClient() {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    try {
      await api.del(clientPath(id, target.id));
      toast.success("Login client deleted");
      if (selected?.id === target.id) setSelected(null);
      loadClients();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    }
  }

  async function confirmDeleteRole() {
    if (!id || !deleteRole) return;
    const key = deleteRole.key;
    setDeleteRole(null);
    try {
      await api.del(`/api/projects/${id}/roles/${encodeURIComponent(key)}`);
      toast.success("Role removed");
      loadRoles();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Remove failed");
    }
  }

  // Render the detail panel from the LIVE list so it reflects edits immediately
  // (the selection state would otherwise hold a stale pre-edit object reference).
  const selectedClient = clients.find((c) => c.id === selected?.id) ?? null;

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6 overflow-auto">
      <div className="space-y-2">
        <Link href="/applications"
          className="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 text-sm">
          <ArrowLeft className="size-4" />
          Applications
        </Link>
        <PageHeader title={project?.name || id}
          description="Login clients, roles, and who can use this application." />
      </div>

      {/* Login clients — master/detail (primary surface). */}
      <div className="flex min-h-0 gap-4">
        <div className="min-w-0 flex-1">
          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
              <CardTitle className="text-base">Login clients</CardTitle>
              <AppFormDialog mode="create" projectId={id} app={null}
                open={createOpen} onOpenChange={setCreateOpen}
                onSaved={loadClients} onSecret={setRevealed} />
            </CardHeader>
            <CardContent>
              {clients.length === 0 ? (
                <p className="text-muted-foreground text-sm">No login clients.</p>
              ) : (
                <ul className="space-y-2.5">
                  {clients.map((c) => {
                    const t = appTypeLabel(c);
                    const isSel = selectedClient?.id === c.id;
                    return (
                      <li key={c.id}>
                        <button type="button" aria-pressed={isSel} onClick={() => setSelected(c)}
                          className={`flex w-full items-center gap-2.5 rounded-md border px-2.5 py-2 text-left transition-colors ${isSel ? "border-primary ring-1 ring-primary" : "hover:bg-muted/50"}`}>
                          <span aria-hidden
                            className="flex size-7 shrink-0 items-center justify-center rounded-md bg-violet-500/10 text-violet-600">
                            <AppWindow className="size-4" />
                          </span>
                          <span className="flex min-w-0 flex-col">
                            <span className="truncate text-sm font-medium">{c.name}</span>
                            <span className="text-muted-foreground truncate font-mono text-xs">
                              {c.oidcConfig?.clientId || "—"}
                            </span>
                          </span>
                          {t && (
                            <span className={`ml-auto inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${APP_TYPE_CHIP[t] ?? "bg-slate-500/10 text-slate-600"}`}>
                              {t}
                            </span>
                          )}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </CardContent>
          </Card>
        </div>

        <DetailPanel open={!!selectedClient} title={selectedClient?.name ?? ""}
          subtitle="Login client" onClose={() => setSelected(null)}>
          {selectedClient && (
            <>
              <PanelSection title="OIDC config">
                <PanelField label="Client ID" mono>{selectedClient.oidcConfig?.clientId || "—"}</PanelField>
                <PanelField label="App type">{appTypeLabel(selectedClient) || "—"}</PanelField>
                <PanelField label="Auth method">{pretty(selectedClient.oidcConfig?.authMethodType) || "—"}</PanelField>
                <PanelField label="Grant types">
                  {selectedClient.oidcConfig?.grantTypes?.length
                    ? selectedClient.oidcConfig.grantTypes.map(pretty).join(", ")
                    : "—"}
                </PanelField>
                <PanelField label="Response types">
                  {selectedClient.oidcConfig?.responseTypes?.length
                    ? selectedClient.oidcConfig.responseTypes.map(pretty).join(", ")
                    : "—"}
                </PanelField>
              </PanelSection>
              <PanelSection title="Redirect URIs">
                {selectedClient.oidcConfig?.redirectUris?.length ? (
                  <ul className="space-y-1">
                    {selectedClient.oidcConfig.redirectUris.map((uri) => (
                      <li key={uri} className="font-mono text-xs break-all">{uri}</li>
                    ))}
                  </ul>
                ) : (
                  <span className="text-muted-foreground text-sm">—</span>
                )}
              </PanelSection>
              <div className="flex flex-wrap gap-2 pt-2">
                <Button variant="outline" size="sm" onClick={() => setEditTarget(selectedClient)}>Edit</Button>
                <Button variant="outline" size="sm" onClick={() => setRotateTarget(selectedClient)}>Rotate secret</Button>
                <Button variant="outline" size="sm" className="text-destructive"
                  onClick={() => setDeleteTarget(selectedClient)}>Delete</Button>
              </div>
            </>
          )}
        </DetailPanel>
      </div>

      {/* Roles + Users (secondary). */}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
            <CardTitle className="text-base">Roles</CardTitle>
            <RoleCreateDialog projectId={id} onCreated={loadRoles} />
          </CardHeader>
          <CardContent>
            {roles.length === 0 ? (
              <p className="text-muted-foreground text-sm">No roles defined.</p>
            ) : (
              <ul className="space-y-2.5">
                {roles.map((r) => (
                  <li key={r.key} className="flex items-start gap-2.5">
                    <span aria-hidden
                      className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-indigo-500/10 text-indigo-600">
                      <ShieldCheck className="size-4" />
                    </span>
                    <span className="flex min-w-0 flex-col">
                      <span className="font-mono text-sm font-medium">{r.key}</span>
                      <span className="text-muted-foreground truncate text-xs">{r.displayName || "—"}</span>
                    </span>
                    {r.group && (
                      <span className="ml-auto inline-flex items-center rounded-full bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
                        {r.group}
                      </span>
                    )}
                    <Button variant="ghost" size="icon-sm"
                      className={`text-muted-foreground hover:text-foreground shrink-0${r.group ? " ml-1.5" : " ml-auto"}`}
                      data-testid="app-role-edit" aria-label={`Edit role ${r.key}`}
                      onClick={() => setEditRole(r)}>
                      <Pencil className="size-4" />
                    </Button>
                    <Button variant="ghost" size="icon-sm"
                      className="text-muted-foreground hover:text-destructive ml-0.5 shrink-0"
                      data-testid="app-role-delete" aria-label={`Delete role ${r.key}`}
                      onClick={() => setDeleteRole(r)}>
                      <Trash2 className="size-4" />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Users</CardTitle>
          </CardHeader>
          <CardContent>
            {grants.length === 0 ? (
              <p className="text-muted-foreground text-sm">No users have access.</p>
            ) : (
              <ul className="space-y-3">
                {grants.map((g, i) => {
                  const name = g.displayName || g.userName || g.userId || "—";
                  const seed = g.userId || g.userName || String(i);
                  return (
                    <li key={g.id || g.grantId || seed} className="space-y-1.5">
                      <span className="flex items-center gap-2.5">
                        <span aria-hidden
                          className={`flex size-7 shrink-0 items-center justify-center rounded-full bg-linear-to-br text-[10px] font-bold text-white ${avatarGradient(seed)}`}>
                          {initials(name)}
                        </span>
                        <span className="flex min-w-0 flex-col">
                          <span className="truncate text-sm">{name}</span>
                          {g.userId && (
                            <span className="text-muted-foreground truncate font-mono text-xs">{g.userId}</span>
                          )}
                        </span>
                      </span>
                      <span className="flex flex-wrap gap-1 pl-9">
                        {(g.roleKeys ?? []).length ? (
                          (g.roleKeys ?? []).map((rk) => (
                            <span key={rk}
                              className="inline-flex items-center rounded-md bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
                              {rk}
                            </span>
                          ))
                        ) : (
                          <span className="text-muted-foreground text-xs">—</span>
                        )}
                      </span>
                    </li>
                  );
                })}
              </ul>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Dialogs. */}
      <AppFormDialog mode="edit" projectId={id} app={editTarget} open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)} onSaved={loadClients} onSecret={setRevealed} />
      <SecretRevealDialog clientId={revealed?.clientId}
        clientSecret={revealed?.clientSecret ?? null} onClose={() => setRevealed(null)} />
      <ConfirmDialog open={!!rotateTarget} onOpenChange={(o) => !o && setRotateTarget(null)}
        title="Rotate client secret?"
        description="A new secret is generated and shown once. Any client still using the old secret will immediately fail authentication until updated."
        confirmLabel="Rotate" onConfirm={confirmRotate} />
      <ConfirmDialog open={!!deleteTarget} onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete login client?"
        description="This removes the OIDC client. Changing or removing it can instantly break a live login for users mid-flow. This cannot be undone."
        confirmLabel="Delete" onConfirm={confirmDeleteClient} />
      <RoleEditDialog role={editRole}
        endpoint={`/api/projects/${id}/roles/${encodeURIComponent(editRole?.key ?? "")}`}
        open={!!editRole} onOpenChange={(o) => !o && setEditRole(null)} onSaved={loadRoles} />
      <ConfirmDialog open={!!deleteRole} onOpenChange={(o) => !o && setDeleteRole(null)}
        title={`Remove role ${deleteRole?.key ?? ""}?`}
        description="This cascades — the role is stripped from every user grant on this application. This cannot be undone."
        confirmLabel="Remove role" onConfirm={confirmDeleteRole} />
    </div>
  );
}
