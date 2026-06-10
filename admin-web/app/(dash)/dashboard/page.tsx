"use client";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import {
  AppWindow, Bot, ChevronRight, KeyRound, ShieldCheck, UserRound,
} from "lucide-react";
import { toast } from "sonner";
import {
  Area, AreaChart, CartesianGrid, Cell, Pie, PieChart, ResponsiveContainer,
  Tooltip, XAxis, YAxis,
} from "recharts";
import { Card } from "@/components/ui/card";
import { PageHeader } from "@/components/shell/PageHeader";
import { api, ApiError } from "@/lib/api";
import type { AuditEvent, EventList, Stats, Status } from "@/lib/types";

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

// Bucket events into 24 hourly bins ending at the current hour ("HH:00").
function bucketHourly(events: AuditEvent[]): { hour: string; events: number }[] {
  const now = new Date();
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate(), now.getHours());
  const bins: { start: number; hour: string; events: number }[] = [];
  for (let i = 23; i >= 0; i--) {
    const start = new Date(end.getTime() - i * 3600_000);
    bins.push({
      start: start.getTime(),
      hour: `${String(start.getHours()).padStart(2, "0")}:00`,
      events: 0,
    });
  }
  for (const e of events) {
    if (!e.creationDate) continue;
    const t = new Date(e.creationDate).getTime();
    if (Number.isNaN(t)) continue;
    for (let i = bins.length - 1; i >= 0; i--) {
      if (t >= bins[i].start && t < bins[i].start + 3600_000) {
        bins[i].events++;
        break;
      }
    }
  }
  return bins.map(({ hour, events: n }) => ({ hour, events: n }));
}

export default function DashboardPage() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [events, setEvents] = useState<AuditEvent[] | null>(null);
  // Charts render client-side only (ResponsiveContainer needs a measured DOM).
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);

  const load = useCallback(async () => {
    try {
      setStats(await api.get<Stats>("/api/stats"));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load dashboard");
      }
    }
    // Capability probe is best-effort: a failure just hides the chart.
    let st: Status | null = null;
    try {
      st = await api.get<Status>("/api/status");
    } catch {
      st = null;
    }
    // FAIL CLOSED: only read the event log when the capability is present.
    if (st?.capabilities?.events) {
      try {
        // Real 24h WINDOW, not "last N events": a bare limit would zero out
        // earlier hours whenever one busy hour eats the whole budget. Ask for
        // everything since 24h ago, ascending, up to the API page cap.
        const from = new Date(Date.now() - 24 * 3600_000).toISOString();
        const list = await api.get<EventList>(
          `/api/events?from=${encodeURIComponent(from)}&asc=true&limit=1000`,
        );
        setEvents(list.result ?? []);
      } catch {
        setEvents(null);
      }
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // null count (its fan-out failed) -> em-dash, never a misleading 0 (§10).
  const show = (n: number | null | undefined) => (n == null ? "—" : String(n));

  const humans = stats?.humans ?? null;
  const machines = stats?.machines ?? null;
  const donutData =
    humans != null || machines != null
      ? [
          { name: "Humans", value: humans ?? 0, color: "#3b82f6" },
          { name: "Machines", value: machines ?? 0, color: "#64748b" },
        ]
      : null;
  const donutTotal = (humans ?? 0) + (machines ?? 0);
  const activity = events ? bucketHourly(events) : null;

  return (
    <div className="space-y-6 px-6 py-6">
      <PageHeader
        title="Dashboard"
        description="People, roles, and apps across every app on the platform."
      />

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-5">
        {CARDS.map(({ key, label, href, Icon, bg, fg }) => (
          <Link key={key + label} href={href} aria-label={label}
            className="group rounded-xl transition-shadow hover:shadow-md">
            <Card className="relative gap-0 p-4">
              <ChevronRight
                className="text-muted-foreground absolute top-4 right-3 size-4 opacity-0 transition-opacity group-hover:opacity-100" />
              <div className={`mb-3 flex h-10 w-10 items-center justify-center rounded-xl ${bg} ${fg}`}>
                <Icon className="h-5 w-5" />
              </div>
              <div className="text-2xl font-bold tracking-tight tabular-nums">
                {stats ? show(stats[key]) : "—"}
              </div>
              <div className="text-muted-foreground text-sm">{label}</div>
            </Card>
          </Link>
        ))}
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Activity over the last 24h */}
        <Card className="gap-4 p-5 lg:col-span-2">
          <div>
            <h2 className="text-sm font-semibold">Activity</h2>
            <p className="text-muted-foreground text-xs">
              Audit events per hour, last 24 hours.
              {events && events.length >= 1000 && (
                <span className="text-amber-600"> Window truncated at 1000 events — busiest hours may under-count.</span>
              )}
            </p>
          </div>
          {mounted && activity ? (
            <ResponsiveContainer width="100%" height={220}>
              <AreaChart data={activity} margin={{ top: 4, right: 8, left: -16, bottom: 0 }}>
                <defs>
                  <linearGradient id="activityFill" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="#5b53e8" stopOpacity={0.35} />
                    <stop offset="100%" stopColor="#5b53e8" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" vertical={false} stroke="var(--border)" />
                <XAxis dataKey="hour" tick={{ fontSize: 11 }} tickLine={false}
                  axisLine={false} interval="preserveStartEnd" minTickGap={32} />
                <YAxis tick={{ fontSize: 11 }} tickLine={false} axisLine={false}
                  allowDecimals={false} width={40} />
                <Tooltip />
                <Area type="monotone" dataKey="events" stroke="#5b53e8"
                  strokeWidth={2} fill="url(#activityFill)" />
              </AreaChart>
            </ResponsiveContainer>
          ) : (
            <p className="text-muted-foreground flex h-[220px] items-center justify-center text-sm">
              Audit events unavailable
            </p>
          )}
        </Card>

        {/* Humans vs machines donut */}
        <Card className="gap-4 p-5">
          <div>
            <h2 className="text-sm font-semibold">Users</h2>
            <p className="text-muted-foreground text-xs">
              Humans vs machine accounts.
            </p>
          </div>
          {mounted && donutData ? (
            <>
              <div className="relative">
                <ResponsiveContainer width="100%" height={170}>
                  <PieChart>
                    <Pie data={donutData} dataKey="value" nameKey="name"
                      innerRadius={55} outerRadius={75} strokeWidth={2}
                      stroke="var(--card)" paddingAngle={2}>
                      {donutData.map((d) => (
                        <Cell key={d.name} fill={d.color} />
                      ))}
                    </Pie>
                    <Tooltip />
                  </PieChart>
                </ResponsiveContainer>
                <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
                  <span className="text-2xl font-bold tabular-nums">{donutTotal}</span>
                  <span className="text-muted-foreground text-xs">total</span>
                </div>
              </div>
              <div className="space-y-1.5">
                {donutData.map((d) => (
                  <div key={d.name} className="flex items-center gap-2 text-sm">
                    <span aria-hidden className="size-2 rounded-full"
                      style={{ backgroundColor: d.color }} />
                    <span className="text-muted-foreground flex-1">{d.name}</span>
                    <span className="font-medium tabular-nums">{d.value}</span>
                  </div>
                ))}
              </div>
            </>
          ) : (
            <p className="text-muted-foreground flex h-[170px] items-center justify-center text-sm">
              Stats unavailable
            </p>
          )}
        </Card>

      </div>

      {stats && !stats.tokenHealthy && (
        <p className="text-sm text-rose-600">
          Service-account token unavailable — counts may be stale.
        </p>
      )}
    </div>
  );
}
