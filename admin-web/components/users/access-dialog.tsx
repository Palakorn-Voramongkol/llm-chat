"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import type {
  AppProject, AppProjectList, GrantList, Role, RoleList, User, UserGrant,
} from "@/lib/types";

// One application's row in the matrix: its roles + the user's current grant on it.
interface AppRow {
  project: AppProject;
  roles: Role[];
  grant: UserGrant | null; // the user's existing grant on this project, if any
}

/** Cross-application access matrix: set, for one user, which applications they
 * can use and with what roles, across ALL apps. Generalizes GrantsDialog (which
 * only covers the home project). */
export function AccessDialog({
  user, open, onOpenChange, onSaved,
}: {
  user: User | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
}) {
  const [rows, setRows] = useState<AppRow[]>([]);
  // projectId -> selected role keys for that app (the desired end-state).
  const [selected, setSelected] = useState<Map<string, Set<string>>>(new Map());
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!user) return;
    setLoading(true);
    try {
      const [projectList, grantList] = await Promise.all([
        api.get<AppProjectList>("/api/projects"),
        api.get<GrantList>(`/api/users/${user.id}/grants`),
      ]);
      const projects = projectList.result ?? [];
      // The user's grant per project (one grant per (user, project)).
      const grantByProject = new Map<string, UserGrant>();
      for (const g of grantList.result ?? []) grantByProject.set(g.projectId, g);
      // Each app's role catalogue, in parallel + best-effort: a failed lookup
      // leaves that app with an empty role list ("no roles defined").
      const built = await Promise.all(
        projects.map(async (p): Promise<AppRow> => {
          let roles: Role[] = [];
          try {
            const rl = await api.get<RoleList>(`/api/projects/${p.id}/roles`);
            roles = rl.result ?? [];
          } catch {
            roles = [];
          }
          return { project: p, roles, grant: grantByProject.get(p.id) ?? null };
        }),
      );
      setRows(built);
      // Seed selection from the user's current grants.
      const sel = new Map<string, Set<string>>();
      for (const r of built) {
        sel.set(r.project.id, new Set(r.grant?.roleKeys ?? []));
      }
      setSelected(sel);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error(e instanceof ApiError ? e.message : "Failed to load access");
      }
    } finally {
      setLoading(false);
    }
  }, [user]);

  useEffect(() => {
    if (open) load();
    else { setRows([]); setSelected(new Map()); }
  }, [open, load]);

  function toggle(projectId: string, key: string, on: boolean) {
    setSelected((prev) => {
      const next = new Map(prev);
      const set = new Set(next.get(projectId) ?? []);
      if (on) set.add(key); else set.delete(key);
      next.set(projectId, set);
      return next;
    });
  }

  async function onSave() {
    if (!user) return;
    setSaving(true);
    const errors: string[] = [];
    try {
      for (const row of rows) {
        const pid = row.project.id;
        const before = new Set(row.grant?.roleKeys ?? []);
        const after = selected.get(pid) ?? new Set<string>();
        // Diff: same role set -> nothing to do for this app.
        const same =
          before.size === after.size &&
          [...after].every((k) => before.has(k));
        if (same) continue;
        const nextKeys = [...after];
        try {
          if (!row.grant && nextKeys.length > 0) {
            // App gained roles, no grant yet -> CREATE (always send projectId).
            await api.post(`/api/users/${user.id}/grants`, {
              projectId: pid, roleKeys: nextKeys,
            });
          } else if (row.grant && nextKeys.length > 0) {
            // Role set changed, grant exists -> REPLACE the role set.
            await api.put(`/api/users/${user.id}/grants/${row.grant.grantId}`, {
              roleKeys: nextKeys,
            });
          } else if (row.grant && nextKeys.length === 0) {
            // App went to zero roles, grant exists -> REVOKE the whole grant.
            await api.del(`/api/users/${user.id}/grants/${row.grant.grantId}`);
          }
          // (no grant && nextKeys empty): can't happen (same would be true).
        } catch (e) {
          errors.push(
            `${row.project.name || pid}: ${e instanceof ApiError ? e.message : "failed"}`,
          );
        }
      }
      if (errors.length) {
        toast.error(`Some apps failed: ${errors.join("; ")}`);
      } else {
        toast.success("App access updated");
      }
      onOpenChange(false);
      onSaved();
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-hidden sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>App access for {user?.userName}</DialogTitle>
          <DialogDescription>
            Choose which applications this user can use and with what roles.
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-[60vh] space-y-4 overflow-auto pr-1">
          {loading ? (
            <p className="text-muted-foreground text-sm">Loading…</p>
          ) : rows.length === 0 ? (
            <p className="text-muted-foreground text-sm">No applications.</p>
          ) : (
            rows.map((row) => {
              const pid = row.project.id;
              const sel = selected.get(pid) ?? new Set<string>();
              return (
                <section key={pid} className="space-y-2 rounded-lg border p-3">
                  <div className="flex items-baseline justify-between gap-2">
                    <h3 className="text-sm font-semibold">
                      {row.project.name || pid}
                    </h3>
                    <span className="font-mono text-xs text-muted-foreground">
                      {pid}
                    </span>
                  </div>
                  {row.roles.length === 0 ? (
                    <p className="text-muted-foreground text-xs">No roles defined.</p>
                  ) : (
                    <div className="space-y-1.5">
                      {row.roles.map((r) => (
                        <label key={r.key} className="flex items-center gap-2 text-sm">
                          <Checkbox
                            checked={sel.has(r.key)}
                            onCheckedChange={(v) => toggle(pid, r.key, v === true)}
                            data-testid={`grant-${pid}-${r.key}`}
                          />
                          <span>
                            {r.displayName}{" "}
                            <span className="text-muted-foreground">({r.key})</span>
                          </span>
                        </label>
                      ))}
                    </div>
                  )}
                </section>
              );
            })
          )}
        </div>
        <DialogFooter>
          <Button onClick={onSave} disabled={saving || loading} data-testid="access-save">
            Save access
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
