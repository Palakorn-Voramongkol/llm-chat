import type { LucideIcon } from "lucide-react";
import {
  Card, CardContent, CardDescription, CardHeader, CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

export interface PolicyRow {
  label: string;
  value: string;
}

// Read-only view of an org policy (design §9): org policies are managed by the
// provisioner out-of-band, so there is NO write path here — only a display and a
// degrade note when the policy could not be read.
export function PolicyCard({
  title, description, available, rows, icon: Icon, iconClass,
}: {
  title: string;
  description: string;
  available: boolean;
  rows: PolicyRow[];
  /** Optional header icon tile (lucide icon + tint classes). */
  icon?: LucideIcon;
  iconClass?: string;
}) {
  return (
    <Card data-testid={`policy-card-${title.toLowerCase().replace(/\s+/g, "-")}`}>
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2.5">
            {Icon && (
              <span aria-hidden
                className={`flex size-8 shrink-0 items-center justify-center rounded-lg ${iconClass ?? "bg-slate-500/10 text-slate-600"}`}>
                <Icon className="size-4" />
              </span>
            )}
            <CardTitle>{title}</CardTitle>
          </div>
          <Badge variant="secondary">Read-only</Badge>
        </div>
        {description ? <CardDescription>{description}</CardDescription> : null}
      </CardHeader>
      <CardContent>
        {available ? (
          <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
            {rows.map((row) => (
              <div key={row.label} className="contents">
                <dt className="text-muted-foreground">{row.label}</dt>
                <dd className="font-medium">{row.value}</dd>
              </div>
            ))}
          </dl>
        ) : (
          <p data-testid="policy-managed-out-of-band" className="text-muted-foreground text-sm">
            Managed out-of-band by the provisioner — not editable here.
          </p>
        )}
      </CardContent>
    </Card>
  );
}
