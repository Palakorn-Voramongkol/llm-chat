"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { buildRoleColumns } from "@/components/roles/columns";
import { CreateRoleDialog } from "@/components/roles/create-role-dialog";
import { HoldersDialog } from "@/components/roles/holders-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { api, ApiError } from "@/lib/api";
import type { Role, RoleList } from "@/lib/types";

export default function RolesPage() {
  const [roles, setRoles] = useState<Role[]>([]);
  const [holdersTarget, setHoldersTarget] = useState<Role | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Role | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.get<RoleList>("/api/roles");
      setRoles(list.result);
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

  const columns = buildRoleColumns({
    onHolders: setHoldersTarget,
    onDelete: setDeleteTarget,
  });

  return (
    <div className="space-y-4 px-6 py-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold">Roles</h1>
          <p className="text-muted-foreground text-sm">
            Project roles and the users who hold them across the platform.
          </p>
        </div>
        <CreateRoleDialog onCreated={load} />
      </div>
      <DataTable columns={columns} data={roles}
        filterColumn="key" filterPlaceholder="Filter by key..."
        emptyMessage="No roles." />
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
