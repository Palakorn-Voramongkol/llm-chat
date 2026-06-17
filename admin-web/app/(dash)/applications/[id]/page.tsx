"use client";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { ArrowLeft, ShieldCheck, AppWindow } from "lucide-react";
import { toast } from "sonner";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/shell/PageHeader";
import { appTypeLabel } from "@/components/apps/columns";
import { avatarGradient, initials } from "@/lib/avatar";
import { api, ApiError } from "@/lib/api";
import type {
  AppProject, AppProjectList, OidcApp, OidcAppList,
  ProjectGrant, ProjectGrantList, Role, RoleList,
} from "@/lib/types";

const APP_TYPE_CHIP: Record<string, string> = {
  NATIVE: "bg-emerald-500/10 text-emerald-700",
  WEB: "bg-blue-500/10 text-blue-700",
  API: "bg-violet-500/10 text-violet-700",
  USER_AGENT: "bg-amber-500/10 text-amber-700",
};

export default function ApplicationDetailPage() {
  const params = useParams<{ id: string }>();
  const id = params.id;
  const [project, setProject] = useState<AppProject | null>(null);
  const [roles, setRoles] = useState<Role[]>([]);
  const [clients, setClients] = useState<OidcApp[]>([]);
  const [grants, setGrants] = useState<ProjectGrant[]>([]);

  const load = useCallback(async () => {
    if (!id) return;
    // The project name (find by id from the projects list).
    try {
      const list = await api.get<AppProjectList>("/api/projects");
      setProject((list.result ?? []).find((p) => p.id === id) ?? null);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load application");
      }
    }
    // Roles / clients / roster — each best-effort and independent.
    try {
      const rl = await api.get<RoleList>(`/api/projects/${id}/roles`);
      setRoles(rl.result ?? []);
    } catch { setRoles([]); }
    try {
      const al = await api.get<OidcAppList>(`/api/projects/${id}/apps`);
      setClients(al.result ?? []);
    } catch { setClients([]); }
    try {
      const gl = await api.get<ProjectGrantList>(`/api/projects/${id}/grants`);
      setGrants(gl.result ?? []);
    } catch { setGrants([]); }
  }, [id]);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6">
      <div className="space-y-2">
        <Link
          href="/applications"
          className="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 text-sm"
        >
          <ArrowLeft className="size-4" />
          Applications
        </Link>
        <PageHeader
          title={project?.name || id}
          description="Roles, login clients, and who can use this application."
        />
      </div>

      <div className="grid min-h-0 flex-1 gap-4 overflow-auto lg:grid-cols-3">
        {/* Roles (read-only — CRUD is P2). */}
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Roles</CardTitle>
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
                      <span className="text-muted-foreground truncate text-xs">
                        {r.displayName || "—"}
                      </span>
                    </span>
                    {r.group && (
                      <span className="ml-auto inline-flex items-center rounded-full bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
                        {r.group}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        {/* Login clients (OIDC apps — read-only). */}
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Login clients</CardTitle>
          </CardHeader>
          <CardContent>
            {clients.length === 0 ? (
              <p className="text-muted-foreground text-sm">No login clients.</p>
            ) : (
              <ul className="space-y-2.5">
                {clients.map((c) => {
                  const t = appTypeLabel(c);
                  return (
                    <li key={c.id} className="flex items-start gap-2.5">
                      <span aria-hidden
                        className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-violet-500/10 text-violet-600">
                        <AppWindow className="size-4" />
                      </span>
                      <span className="flex min-w-0 flex-col">
                        <span className="truncate text-sm font-medium">{c.name}</span>
                        <span className="text-muted-foreground truncate font-mono text-xs">
                          {c.oidcConfig?.clientId || "—"}
                        </span>
                      </span>
                      {t && (
                        <span
                          className={`ml-auto inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${APP_TYPE_CHIP[t] ?? "bg-slate-500/10 text-slate-600"}`}
                        >
                          {t}
                        </span>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
          </CardContent>
        </Card>

        {/* Users roster — who can use this app + their roles here. */}
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
                            <span className="text-muted-foreground truncate font-mono text-xs">
                              {g.userId}
                            </span>
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
    </div>
  );
}
