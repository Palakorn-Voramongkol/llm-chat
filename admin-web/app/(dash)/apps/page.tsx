"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { PageHeader } from "@/components/shell/PageHeader";
import { buildAppColumns } from "@/components/apps/columns";
import { AppFormDialog } from "@/components/apps/app-form-dialog";
import { SecretRevealDialog } from "@/components/apps/secret-reveal-dialog";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { api, ApiError } from "@/lib/api";
import type { OidcApp, OidcAppList, AppSecret } from "@/lib/types";

export default function ApplicationsPage() {
  const [apps, setApps] = useState<OidcApp[]>([]);
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<OidcApp | null>(null);
  const [rotateTarget, setRotateTarget] = useState<OidcApp | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<OidcApp | null>(null);
  const [revealed, setRevealed] = useState<AppSecret | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.get<OidcAppList>("/api/apps");
      setApps(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load applications");
      }
    }
  }, []);

  useEffect(() => {
    api.get("/api/me").catch(() => {});
    load();
  }, [load]);

  async function confirmRotate() {
    if (!rotateTarget) return;
    try {
      const s = await api.post<AppSecret>(`/api/apps/${rotateTarget.id}/secret`);
      toast.success("Secret rotated");
      if (s?.clientSecret) setRevealed(s); // one-time reveal
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Rotate failed");
    } finally {
      setRotateTarget(null);
    }
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      await api.del(`/api/apps/${deleteTarget.id}`);
      toast.success("Application deleted");
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    } finally {
      setDeleteTarget(null);
      load();
    }
  }

  const columns = buildAppColumns({
    onEdit: setEditTarget,
    onRotate: setRotateTarget,
    onDelete: setDeleteTarget,
  });

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6">
      <PageHeader
        title="Applications"
        description="OIDC clients registered against the platform project."
        actions={
          <AppFormDialog mode="create" app={null} open={createOpen} onOpenChange={setCreateOpen}
            onSaved={load} onSecret={setRevealed} />
        }
      />
      <div className="flex-1 min-h-0">
        <DataTable columns={columns} data={apps}
          filterColumn="name" filterPlaceholder="Filter by name..."
          emptyMessage="No applications." />
      </div>
      <AppFormDialog mode="edit" app={editTarget} open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)} onSaved={load} onSecret={setRevealed} />
      <SecretRevealDialog clientId={revealed?.clientId}
        clientSecret={revealed?.clientSecret ?? null} onClose={() => setRevealed(null)} />
      <ConfirmDialog open={!!rotateTarget}
        onOpenChange={(o) => !o && setRotateTarget(null)}
        title="Rotate client secret?"
        description="A new secret is generated and shown once. Any client still using the old secret will immediately fail authentication until updated."
        confirmLabel="Rotate" onConfirm={confirmRotate} />
      <ConfirmDialog open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete application?"
        description="This removes the OIDC client. Changing or removing redirectUris can instantly break a live login for users mid-flow. This cannot be undone."
        confirmLabel="Delete" onConfirm={confirmDelete} />
    </div>
  );
}
