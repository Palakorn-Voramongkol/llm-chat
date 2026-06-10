"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { PageHeader } from "@/components/shell/PageHeader";
import { buildColumns, type Lifecycle } from "@/components/users/columns";
import { CreateUserDialog } from "@/components/users/create-user-dialog";
import { EditUserDialog } from "@/components/users/edit-user-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { GrantsDialog } from "@/components/users/grants-dialog";
import { api, ApiError } from "@/lib/api";
import type { User, UserList } from "@/lib/types";

export default function UsersPage() {
  const [users, setUsers] = useState<User[]>([]);
  const [editTarget, setEditTarget] = useState<User | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);
  const [grantsTarget, setGrantsTarget] = useState<User | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.get<UserList>("/api/users");
      setUsers(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load users");
      }
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
        actions={<CreateUserDialog onCreated={load} />}
      />
      <div className="flex-1 min-h-0">
        <DataTable columns={columns} data={users}
          filterColumn="userName" filterPlaceholder="Filter by username..."
          emptyMessage="No users." />
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
