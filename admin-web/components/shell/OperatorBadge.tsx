"use client";
import { useEffect, useState } from "react";
import { LogOut } from "lucide-react";
import { api } from "@/lib/api";
import { initials } from "@/lib/avatar";
import type { Me } from "@/lib/types";

// Re-exported for callers/tests that import `initials` from here; the
// canonical implementation lives in lib/avatar.ts.
export { initials };

export function OperatorBadge() {
  const [me, setMe] = useState<Me | null>(null);

  useEffect(() => {
    // 401 inside lib/api full-page-redirects to /login; swallow here (spec §4).
    api.get<Me>("/api/me").then(setMe).catch(() => {});
  }, []);

  return (
    <div className="flex items-center gap-3">
      <span className="flex items-center gap-2 text-sm font-semibold">
        <span className="flex size-8 items-center justify-center rounded-full bg-linear-to-br from-indigo-500 to-violet-500 text-xs font-bold text-white">
          {initials(me?.name ?? "")}
        </span>
        {me?.name ?? "—"}
      </span>
      <a
        href="/logout"
        className="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 text-sm"
      >
        <LogOut className="size-4" />
        <span>Sign out</span>
      </a>
    </div>
  );
}
