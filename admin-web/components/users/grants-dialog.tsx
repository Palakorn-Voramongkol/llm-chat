"use client";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import type { GrantList, Role, RoleList, User, UserGrant } from "@/lib/types";

export function GrantsDialog({
  user, open, onOpenChange, onSaved,
}: {
  user: User | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
}) {
  const [roles, setRoles] = useState<Role[]>([]);
  const [grant, setGrant] = useState<UserGrant | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!user) return;
    try {
      const [roleList, grantList] = await Promise.all([
        api.get<RoleList>("/api/roles"),
        api.get<GrantList>(`/api/users/${user.id}/grants`),
      ]);
      setRoles(roleList.result);
      // One grant per (user, project): the first (only) grant, if any.
      const g = grantList.result[0] ?? null;
      setGrant(g);
      setSelected(new Set(g?.roleKeys ?? []));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error(e instanceof ApiError ? e.message : "Failed to load grants");
      }
    }
  }, [user]);

  useEffect(() => {
    if (open) load();
    else { setRoles([]); setGrant(null); setSelected(new Set()); }
  }, [open, load]);

  function toggle(key: string, on: boolean) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (on) next.add(key); else next.delete(key);
      return next;
    });
  }

  const nextKeys = useMemo(() => Array.from(selected), [selected]);

  async function onSave() {
    if (!user) return;
    setSaving(true);
    try {
      // The one-grant-per-project branch (design §7):
      if (!grant && nextKeys.length > 0) {
        // No grant yet, roles chosen -> POST create.
        await api.post(`/api/users/${user.id}/grants`, { roleKeys: nextKeys });
      } else if (grant && nextKeys.length > 0) {
        // Grant exists, roles chosen -> PUT replace the whole roleKeys set.
        await api.put(`/api/users/${user.id}/grants/${grant.grantId}`, { roleKeys: nextKeys });
      } else if (grant && nextKeys.length === 0) {
        // Grant exists, nothing chosen -> DELETE revoke the whole grant.
        await api.del(`/api/users/${user.id}/grants/${grant.grantId}`);
      }
      // (no grant && nothing chosen): nothing to do.
      toast.success("Access updated");
      onOpenChange(false);
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Access for {user?.userName}</DialogTitle>
        </DialogHeader>
        <div className="space-y-2">
          {roles.length === 0 ? (
            <p className="text-sm text-muted-foreground">No roles defined.</p>
          ) : roles.map((r) => (
            <label key={r.key} className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={selected.has(r.key)}
                onCheckedChange={(v) => toggle(r.key, v === true)}
                data-testid={`grant-role-${r.key}`}
              />
              <span>{r.displayName} <span className="text-muted-foreground">({r.key})</span></span>
            </label>
          ))}
        </div>
        <DialogFooter>
          <Button onClick={onSave} disabled={saving} data-testid="grants-save">
            Save access
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
