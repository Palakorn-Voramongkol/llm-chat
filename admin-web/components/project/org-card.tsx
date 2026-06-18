"use client";
import { useEffect, useState } from "react";
import { Building2 } from "lucide-react";
import { toast } from "sonner";
import {
  Card, CardContent, CardDescription, CardHeader, CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { api, ApiError } from "@/lib/api";
import type { Org } from "@/lib/types";

// The platform organization (design §9). Editable: the runtime SA holds
// ORG_SETTINGS_MANAGER (minimal org.write role — NOT ORG_OWNER), so the Console
// can rename the org. Org POLICIES remain provisioner-managed (read-only).
export function OrgCard({ org, onSaved }: { org: Org | null; onSaved?: () => void }) {
  const [name, setName] = useState(org?.name ?? "");
  const [saving, setSaving] = useState(false);

  // Re-seed the field whenever the loaded org changes.
  useEffect(() => {
    setName(org?.name ?? "");
  }, [org?.name]);

  const trimmed = name.trim();
  const dirty = !!org && trimmed.length > 0 && trimmed !== (org.name ?? "");

  async function save() {
    if (!dirty) return;
    setSaving(true);
    try {
      await api.put("/api/org", { name: trimmed });
      toast.success("Organization renamed");
      onSaved?.();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Rename failed");
    } finally {
      setSaving(false);
    }
  }

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
          The platform organization that owns every project, user, and policy.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="space-y-2">
          <Label htmlFor="org-name">Name</Label>
          <Input
            id="org-name"
            data-testid="org-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Organization name"
          />
        </div>
        <p className="text-muted-foreground font-mono text-xs">{org?.id ?? "—"}</p>
        <Button data-testid="org-save" onClick={save} disabled={!dirty || saving}>
          {saving ? "Saving…" : "Save"}
        </Button>
      </CardContent>
    </Card>
  );
}
