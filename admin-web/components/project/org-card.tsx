import { Building2 } from "lucide-react";
import {
  Card, CardContent, CardDescription, CardHeader, CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { Org } from "@/lib/types";

// Read-only view of the platform organization (design §9). The Console's service
// account is least-privilege and CANNOT rename the org (that needs ORG_OWNER), so
// there is NO write path here — only a display of the name + id and a runbook note.
export function OrgCard({ org }: { org: Org | null }) {
  return (
    <Card data-testid="org-card">
      <CardHeader>
        <div className="flex items-center gap-2.5">
          <span aria-hidden
            className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-indigo-500/10 text-indigo-600">
            <Building2 className="size-4" />
          </span>
          <CardTitle>Organization</CardTitle>
        </div>
        <CardDescription>
          The platform organization that owns the project, users, and policies.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="space-y-2">
          <Label htmlFor="org-name">Name</Label>
          <Input
            id="org-name"
            data-testid="org-name"
            value={org?.name ?? "—"}
            readOnly
            disabled
          />
        </div>
        <p className="text-muted-foreground font-mono text-xs">{org?.id ?? "—"}</p>
        <p className="text-muted-foreground text-sm">
          Renaming the organization requires ORG_OWNER. The Console&apos;s service
          account is least-privilege, so rename it with the runbook:{" "}
          <code className="bg-muted rounded px-1 py-0.5 font-mono text-xs">
            docker compose run --rm -e ORG_NAME=&quot;…&quot; --entrypoint python
            zitadel-init /app/org_rename.py
          </code>
        </p>
      </CardContent>
    </Card>
  );
}
