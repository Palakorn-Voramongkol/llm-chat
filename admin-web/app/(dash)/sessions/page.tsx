"use client";
import { useCallback, useEffect, useState } from "react";
import { LogOut, MessageSquare } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { PageHeader } from "@/components/shell/PageHeader";
import { api, ApiError } from "@/lib/api";
import { avatarGradient, initials } from "@/lib/avatar";
import { eventChipClass } from "@/lib/event-style";
import type { ChatSessions, SigninList, Status } from "@/lib/types";
import { toast } from "sonner";

// "in 2h 5m" countdown computed once from Date.now at render (no ticking).
function expiresIn(expiresAt: string): string {
  const ms = new Date(expiresAt).getTime() - Date.now();
  if (Number.isNaN(ms)) return "";
  if (ms <= 0) return "expired";
  const mins = Math.floor(ms / 60_000);
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return h > 0 ? `in ${h}h ${m}m` : `in ${m}m`;
}

function roleChipClass(role: string): string {
  return role === "chat.admin"
    ? "bg-indigo-500/10 text-indigo-600"
    : "bg-slate-500/10 text-slate-600";
}

function HealthRow({
  ok, label, detail,
}: {
  ok: "ok" | "down" | "warn";
  label: string;
  detail: string;
}) {
  const dot =
    ok === "ok" ? "bg-emerald-500" : ok === "down" ? "bg-rose-500" : "bg-amber-500";
  return (
    <div className="flex items-start gap-2.5 text-sm">
      <span aria-hidden className={`mt-1.5 size-2 shrink-0 rounded-full ${dot}`} />
      <div>
        <div className="font-medium">{label}</div>
        <div className="text-muted-foreground text-xs">{detail}</div>
      </div>
    </div>
  );
}

// byBackend values are {sessions:[...]} objects keyed by port — read defensively.
function backendOf(sessionId: string, byBackend?: Record<string, unknown>): string {
  if (!byBackend) return "—";
  for (const [port, v] of Object.entries(byBackend)) {
    const sessions = (v as { sessions?: unknown })?.sessions;
    if (Array.isArray(sessions) && sessions.includes(sessionId)) return port;
  }
  return "—";
}

export default function SessionsPage() {
  const [status, setStatus] = useState<Status | null>(null);
  const [chat, setChat] = useState<ChatSessions | null>(null);
  const [signins, setSignins] = useState<SigninList | null>(null);

  const load = useCallback(async () => {
    try {
      setStatus(await api.get<Status>("/api/status"));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load session status");
      }
    }
    // Best-effort panels: each failure degrades its own card, never the page.
    try {
      setChat(await api.get<ChatSessions>("/api/chat-sessions"));
    } catch {
      setChat(null);
    }
    try {
      setSignins(await api.get<SigninList>("/api/signins"));
    } catch {
      setSignins(null);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const op = status?.operator;
  const expiresAt = status?.session.expiresAt ?? null;
  const sessionIds = chat?.list?.sessions ?? [];
  const perPort = chat?.instances?.sessionsPerPort ?? {};
  const signinEvents = signins?.available ? (signins.result ?? []).slice(0, 8) : [];

  return (
    <div className="space-y-6 px-6 py-6">
      <PageHeader
        title="Sessions"
        description="Your operator session, live chat sessions, and recent sign-ins."
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Your session */}
        <Card className="gap-4 p-5">
          <h2 className="text-sm font-semibold">Your session</h2>
          {op ? (
            <div className="space-y-4">
              <div className="flex items-center gap-3">
                <span aria-hidden
                  className={`flex size-11 shrink-0 items-center justify-center rounded-full bg-linear-to-br text-sm font-bold text-white ${avatarGradient(op.userId || op.name)}`}>
                  {initials(op.name)}
                </span>
                <div className="min-w-0">
                  <div className="font-medium">{op.name}</div>
                  <div className="font-mono text-xs text-muted-foreground">{op.userId}</div>
                </div>
              </div>
              <div className="flex flex-wrap gap-1.5">
                {op.roles.map((r) => (
                  <span key={r}
                    className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${roleChipClass(r)}`}>
                    {r}
                  </span>
                ))}
              </div>
              <div className="text-sm">
                <div className="text-muted-foreground text-xs font-semibold tracking-wide uppercase">
                  Session expires
                </div>
                {expiresAt ? (
                  <div className="mt-0.5">
                    {new Date(expiresAt).toLocaleString()}{" "}
                    <span className="text-muted-foreground text-xs">
                      {expiresIn(expiresAt)}
                    </span>
                  </div>
                ) : (
                  <div className="text-muted-foreground mt-0.5">—</div>
                )}
              </div>
              <Button variant="outline" asChild>
                <a href="/logout">
                  <LogOut className="size-4" />
                  Sign out
                </a>
              </Button>
            </div>
          ) : (
            <p className="text-muted-foreground text-sm">Session status unavailable.</p>
          )}
        </Card>

        {/* Platform health */}
        <Card className="gap-4 p-5">
          <h2 className="text-sm font-semibold">Platform health</h2>
          <div className="space-y-3">
            <HealthRow
              ok={status ? (status.health.zitadel ? "ok" : "down") : "warn"}
              label={`Zitadel ${status ? (status.health.zitadel ? "— Operational" : "— Unreachable") : ""}`}
              detail="Identity provider behind every login and admin call."
            />
            <HealthRow
              ok={status?.capabilities.events ? "ok" : "warn"}
              label={`Audit capability ${status?.capabilities.events ? "on" : "off"}`}
              detail="Event log + sign-in history need IAM_OWNER_VIEWER on the service account."
            />
            <HealthRow
              ok={status?.capabilities.chatSessions ? "ok" : "warn"}
              label={`Chat sessions ${status?.capabilities.chatSessions ? "configured" : "not configured"}`}
              detail="Live session panel needs MANAGER_CONTROL_URL on the BFF."
            />
          </div>
        </Card>

        {/* Recent sign-ins */}
        <Card className="gap-4 p-5">
          <h2 className="text-sm font-semibold">Recent sign-ins</h2>
          {signins && signins.available ? (
            signinEvents.length ? (
              <ul className="space-y-2.5">
                {signinEvents.map((e, i) => (
                  <li key={`${e.sequence ?? i}`} className="flex items-center gap-2 text-sm">
                    <span className={eventChipClass(e.type?.type)}>
                      {e.type?.localized?.localizedMessage ?? e.type?.type ?? "—"}
                    </span>
                    <span className="font-mono text-xs text-muted-foreground">
                      {e.aggregate?.id ?? "—"}
                    </span>
                    <span className="text-muted-foreground ml-auto text-xs whitespace-nowrap">
                      {e.creationDate ? new Date(e.creationDate).toLocaleString() : "—"}
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-muted-foreground text-sm">No recent sign-ins.</p>
            )
          ) : (
            <p className="text-muted-foreground text-sm">
              Sign-in history requires the audit capability.
            </p>
          )}
        </Card>

        {/* Active chat sessions */}
        <Card className="gap-4 p-5 lg:col-span-3">
          <div className="flex items-center gap-2.5">
            <span aria-hidden
              className="flex size-8 items-center justify-center rounded-lg bg-indigo-500/10 text-indigo-600">
              <MessageSquare className="size-4" />
            </span>
            <h2 className="text-sm font-semibold">Active chat sessions</h2>
          </div>
          {!chat || !chat.configured ? (
            <p className="text-muted-foreground text-sm">
              Chat-sessions panel is not configured (MANAGER_CONTROL_URL).
            </p>
          ) : chat.ok === false ? (
            <p className="text-muted-foreground text-sm">
              Manager unreachable{chat.error ? ` — ${chat.error}` : "."}
            </p>
          ) : (
            <div className="space-y-4">
              <div className="flex flex-wrap items-center gap-3">
                <span className="text-2xl font-bold tabular-nums">
                  {chat.list?.count ?? sessionIds.length}
                </span>
                <span className="text-muted-foreground text-sm">active now</span>
                <span className="flex flex-wrap gap-1.5">
                  {Object.entries(perPort).map(([port, n]) => (
                    <span key={port}
                      className="inline-flex items-center rounded-full bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
                      :{port} → {n} session{n === 1 ? "" : "s"}
                    </span>
                  ))}
                </span>
              </div>
              {sessionIds.length ? (
                <div className="overflow-auto rounded-xl border">
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b">
                        <th className="text-muted-foreground px-3 py-2 text-left text-xs font-semibold tracking-wide uppercase">
                          Session ID
                        </th>
                        <th className="text-muted-foreground px-3 py-2 text-left text-xs font-semibold tracking-wide uppercase">
                          Backend port
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {sessionIds.map((sid) => (
                        <tr key={sid} className="hover:bg-muted/50 border-b last:border-0">
                          <td className="px-3 py-2 font-mono text-xs">{sid}</td>
                          <td className="px-3 py-2 font-mono text-xs text-muted-foreground">
                            {backendOf(sid, chat.list?.byBackend)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <p className="text-muted-foreground text-sm">No active sessions.</p>
              )}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
