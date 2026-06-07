"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { DataTable } from "@/components/ui/data-table";
import { buildColumns, type Lifecycle } from "@/components/users/columns";
import { CreateUserDialog } from "@/components/users/create-user-dialog";
import { EditUserDialog } from "@/components/users/edit-user-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { api, ApiError } from "@/lib/api";
import type { Me, User, UserList } from "@/lib/types";

export default function UsersPage() {
  const [me, setMe] = useState<Me | null>(null);
  const [users, setUsers] = useState<User[]>([]);
  const [editTarget, setEditTarget] = useState<User | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<User | null>(null);

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
    // /api/me gate: 401 inside lib/api redirects to /login (full-page nav)
    api.get<Me>("/api/me").then(setMe).catch(() => {});
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
  });

  return (
    <main className="container mx-auto py-8 space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">Users</h1>
          {me && <p className="text-sm text-muted-foreground">Signed in as {me.name}</p>}
        </div>
        <div className="flex gap-2">
          <CreateUserDialog onCreated={load} />
          <Button variant="outline" asChild><a href="/logout">Sign out</a></Button>
        </div>
      </div>
      <DataTable columns={columns} data={users}
        filterColumn="userName" filterPlaceholder="Filter by username..." />
      <EditUserDialog user={editTarget} open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)} onSaved={load} />
      <ConfirmDialog open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete user?"
        description="This is irreversible and removes the user and any machine keys. Already-issued tokens stay valid until their TTL expires."
        confirmLabel="Delete" onConfirm={confirmDelete} />
    </main>
  );
}
