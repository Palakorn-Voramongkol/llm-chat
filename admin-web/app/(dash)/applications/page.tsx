"use client";
import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { DataTable } from "@/components/ui/data-table";
import { PageHeader } from "@/components/shell/PageHeader";
import { buildApplicationColumns, type AppMeta } from "@/components/applications/columns";
import { api, ApiError } from "@/lib/api";
import type {
  AppProject, AppProjectList, OidcAppList, ProjectGrantList, RoleList,
} from "@/lib/types";

export default function ApplicationsPage() {
  const router = useRouter();
  const [apps, setApps] = useState<AppProject[]>([]);
  // projectId -> { role keys, distinct grant-holder count } (best-effort).
  const [metaById, setMetaById] = useState<Map<string, AppMeta>>(new Map());

  const load = useCallback(async () => {
    let projects: AppProject[] = [];
    try {
      const list = await api.get<AppProjectList>("/api/projects");
      projects = list.result ?? [];
      setApps(projects);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load applications");
      }
      return;
    }
    // Per-app #roles + #users in parallel + best-effort (like roles/page.tsx):
    // a failed lookup simply omits that app from the meta map (cell shows "…").
    const pairs = await Promise.all(
      projects.map(async (p): Promise<[string, AppMeta] | null> => {
        try {
          const [roles, grants] = await Promise.all([
            api.get<RoleList>(`/api/projects/${p.id}/roles`),
            api.get<ProjectGrantList>(`/api/projects/${p.id}/grants`),
          ]);
          const userCount = new Set(
            (grants.result ?? [])
              .map((g) => g.userId)
              .filter((u): u is string => !!u),
          ).size;
          // Client count is its own best-effort fetch: a clients-list permission
          // error must not blank this app's roles/users counts.
          let clientCount = 0;
          try {
            const apps = await api.get<OidcAppList>(`/api/projects/${p.id}/apps`);
            clientCount = (apps.result ?? []).length;
          } catch { /* leave clientCount at 0 */ }
          return [p.id, {
            roleKeys: (roles.result ?? []).map((r) => r.key),
            userCount,
            clientCount,
          }];
        } catch {
          return null;
        }
      }),
    );
    setMetaById(new Map(pairs.filter((p): p is [string, AppMeta] => p !== null)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const columns = buildApplicationColumns(metaById);

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6">
      <PageHeader
        title="Applications"
        description="Applications on the platform — each defines its own roles and the users who can use it."
      />
      <div className="min-h-0 flex-1">
        <DataTable columns={columns} data={apps}
          filterFields={[
            { column: "name", label: "Name", placeholder: "Search name…" },
          ]}
          emptyMessage="No applications."
          getRowId={(a) => a.id}
          onRowClick={(a) => router.push(`/applications/${a.id}`)} />
      </div>
    </div>
  );
}
