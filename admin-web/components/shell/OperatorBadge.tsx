"use client";
import { useEffect, useState } from "react";
import { LogOut } from "lucide-react";
import { api } from "@/lib/api";
import type { Me } from "@/lib/types";

/** First letter of up to two words, uppercased; "?" when empty. */
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  return parts.slice(0, 2).map((p) => p[0]!.toUpperCase()).join("");
}

export function OperatorBadge() {
  const [me, setMe] = useState<Me | null>(null);

  useEffect(() => {
    // 401 inside lib/api full-page-redirects to /login; swallow here (spec §4).
    api.get<Me>("/api/me").then(setMe).catch(() => {});
  }, []);

  return (
    <div className="flex items-center gap-3">
      <span className="flex items-center gap-2 text-sm font-semibold">
        <span className="flex size-8 items-center justify-center rounded-full bg-gradient-to-br from-indigo-500 to-violet-500 text-xs font-bold text-white">
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
