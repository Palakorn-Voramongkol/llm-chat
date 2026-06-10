"use client";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { UserRound, Bot, ShieldCheck, KeyRound, AppWindow } from "lucide-react";
import { toast } from "sonner";
import { Card } from "@/components/ui/card";
import { api, ApiError } from "@/lib/api";
import type { Stats } from "@/lib/types";

// Mockup tints (docs/superpowers/specs/mockups/console-shell.html): each card's
// icon sits on a translucent brand wash. bg/fg = [icon bg, icon fg].
type CardDef = {
  key: keyof Omit<Stats, "tokenHealthy">;
  label: string;
  href: string;
  Icon: typeof UserRound;
  bg: string;
  fg: string;
};

const CARDS: CardDef[] = [
  { key: "humans",   label: "Humans",           href: "/users",  Icon: UserRound,   bg: "bg-blue-500/12",    fg: "text-blue-600" },
  { key: "machines", label: "Machine accounts", href: "/users",  Icon: Bot,         bg: "bg-cyan-500/14",    fg: "text-cyan-600" },
  { key: "roles",    label: "Roles",            href: "/roles",  Icon: ShieldCheck, bg: "bg-indigo-500/12",  fg: "text-indigo-600" },
  { key: "grants",   label: "Grants",           href: "/users",  Icon: KeyRound,    bg: "bg-emerald-500/12", fg: "text-emerald-600" },
  { key: "apps",     label: "Apps",             href: "/apps",   Icon: AppWindow,   bg: "bg-violet-500/14",  fg: "text-violet-600" },
];

export default function DashboardPage() {
  const [stats, setStats] = useState<Stats | null>(null);

  const load = useCallback(async () => {
    try {
      setStats(await api.get<Stats>("/api/stats"));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load dashboard");
      }
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // null count (its fan-out failed) -> em-dash, never a misleading 0 (§10).
  const show = (n: number | null | undefined) => (n == null ? "—" : String(n));

  return (
    <div className="space-y-6 px-6 py-6">
      <div>
        <h1 className="text-xl font-bold">Dashboard</h1>
        <p className="text-muted-foreground text-sm">
          People, roles, and apps across every app on the platform.
        </p>
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-5">
        {CARDS.map(({ key, label, href, Icon, bg, fg }) => (
          <Link key={key + label} href={href} aria-label={label}
            className="rounded-xl transition-shadow hover:shadow-md">
            <Card className="gap-0 p-4">
              <div className={`mb-3 flex h-10 w-10 items-center justify-center rounded-xl ${bg} ${fg}`}>
                <Icon className="h-5 w-5" />
              </div>
              <div className="text-2xl font-bold tracking-tight">
                {stats ? show(stats[key]) : "—"}
              </div>
              <div className="text-muted-foreground text-sm">{label}</div>
            </Card>
          </Link>
        ))}
      </div>
      {stats && !stats.tokenHealthy && (
        <p className="text-sm text-rose-600">
          Service-account token unavailable — counts may be stale.
        </p>
      )}
    </div>
  );
}
