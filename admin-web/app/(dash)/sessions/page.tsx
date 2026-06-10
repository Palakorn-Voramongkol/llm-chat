"use client";
import { useCallback, useEffect, useState } from "react";
import { LogOut, Server } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { PageHeader } from "@/components/shell/PageHeader";
import { api, ApiError } from "@/lib/api";
import { avatarGradient, initials } from "@/lib/avatar";
import { eventChipClass, eventLabel } from "@/lib/event-style";
import type { ChatSessions, SigninList, Status, UserList } from "@/lib/types";
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

// One worker row: derived from the manager's byBackend (the worker's own
// `list` reply — {sessions:[...]} when reachable, {error} when down).
interface WorkerRow {
  port: string;
  ok: boolean;
  error?: string;
  sids: string[];
}

function deriveWorkers(chat: ChatSessions | null): WorkerRow[] {
  const byBackend = (chat?.list?.byBackend ?? {}) as Record<string, unknown>;
  // The instances reply is the authoritative worker list; byBackend keys are
  // the fallback when instances degraded.
  const ports =
    chat?.instances?.ports?.map(String) ?? Object.keys(byBackend);
  return [...new Set(ports)].map((port) => {
    const raw = byBackend[port] as { sessions?: unknown; error?: unknown } | undefined;
    const ok = Array.isArray(raw?.sessions);
    return {
      port,
      ok,
      error: typeof raw?.error === "string" ? raw.error : undefined,
      sids: ok ? (raw!.sessions as string[]) : [],
    };
  });
}

export default function SessionsPage() {
  const [status, setStatus] = useState<Status | null>(null);
  const [chat, setChat] = useState<ChatSessions | null>(null);
  const [signins, setSignins] = useState<SigninList | null>(null);
  const [usersById, setUsersById] = useState<Map<string, string>>(new Map());

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
    // Owner display names for the workers panel (userId -> userName).
    try {
      const ul = await api.get<UserList>("/api/users");
      setUsersById(new Map((ul.result ?? []).map((u) => [u.id, u.userName])));
    } catch {
      setUsersById(new Map());
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const op = status?.operator;
  const expiresAt = status?.session.expiresAt ?? null;
  const signinEvents = signins?.available ? (signins.result ?? []).slice(0, 8) : [];
  const workers = deriveWorkers(chat);
  const clientsBySid = new Map(
    (chat?.clients?.clients ?? []).map((c) => [c.sid, c]),
  );

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
                      {eventLabel(e.type)}
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

        {/* Workers & their sessions */}
        <Card className="gap-4 p-5 lg:col-span-3">
          <div className="flex items-center gap-2.5">
            <span aria-hidden
              className="flex size-8 items-center justify-center rounded-lg bg-indigo-500/10 text-indigo-600">
              <Server className="size-4" />
            </span>
            <div>
              <h2 className="text-sm font-semibold">Workers</h2>
              <p className="text-muted-foreground text-xs">
                Each worker, its status, and whose sessions it is running.
              </p>
            </div>
          </div>
          {!chat || !chat.configured ? (
            <p className="text-muted-foreground text-sm">
              Workers panel is not configured (MANAGER_CONTROL_URL).
            </p>
          ) : chat.ok === false ? (
            <p className="text-muted-foreground text-sm">
              Manager unreachable{chat.error ? ` — ${chat.error}` : "."}
            </p>
          ) : workers.length === 0 ? (
            <p className="text-muted-foreground text-sm">No workers reported by the manager.</p>
          ) : (
            <div className="space-y-3">
              {workers.map((w) => (
                <div key={w.port} className="rounded-xl border">
                  <div className="flex flex-wrap items-center gap-2.5 border-b px-4 py-2.5">
                    <span aria-hidden
                      className={`size-2 rounded-full ${w.ok ? "bg-emerald-500" : "bg-rose-500"}`} />
                    <span className="text-sm font-medium">Worker :{w.port}</span>
                    <span className={`text-xs ${w.ok ? "text-muted-foreground" : "text-rose-600"}`}>
                      {w.ok
                        ? `online — ${w.sids.length} session${w.sids.length === 1 ? "" : "s"}`
                        : `unreachable${w.error ? ` — ${w.error}` : ""}`}
                    </span>
                  </div>
                  {w.ok && (w.sids.length ? (
                    <table className="w-full text-sm">
                      <thead>
                        <tr className="border-b">
                          {["Session ID", "Owner", "Questions", "Connected", "Last activity"].map((h) => (
                            <th key={h}
                              className="text-muted-foreground px-4 py-2 text-left text-xs font-semibold tracking-wide uppercase">
                              {h}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {w.sids.map((sid) => {
                          const c = clientsBySid.get(sid);
                          const owner = c ? usersById.get(c.userId) : undefined;
                          return (
                            <tr key={sid} className="hover:bg-muted/50 border-b last:border-0">
                              <td className="px-4 py-2 font-mono text-xs">{sid}</td>
                              <td className="px-4 py-2">
                                {c ? (
                                  <span className="flex items-baseline gap-1.5">
                                    <span className="text-sm font-medium">{owner ?? "—"}</span>
                                    <span className="font-mono text-xs text-muted-foreground">{c.userId}</span>
                                  </span>
                                ) : (
                                  <span className="text-muted-foreground text-xs">
                                    no live client (idle session)
                                  </span>
                                )}
                              </td>
                              <td className="px-4 py-2 text-sm tabular-nums">{c?.questionsSent ?? "—"}</td>
                              <td className="px-4 py-2 text-xs text-muted-foreground">
                                {c?.connectedAt ? new Date(c.connectedAt).toLocaleString() : "—"}
                              </td>
                              <td className="px-4 py-2 text-xs text-muted-foreground">
                                {c?.lastQAt ? new Date(c.lastQAt).toLocaleString() : "—"}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  ) : (
                    <p className="text-muted-foreground px-4 py-3 text-sm">No active sessions.</p>
                  ))}
                </div>
              ))}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
