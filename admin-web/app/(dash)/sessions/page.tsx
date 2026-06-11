"use client";
import { useCallback, useEffect, useState } from "react";
import { LogIn, MessageSquare, Server, Users as UsersIcon } from "lucide-react";
import { Card } from "@/components/ui/card";
import { PageHeader } from "@/components/shell/PageHeader";
import { api, ApiError } from "@/lib/api";
import { eventChipClass, eventLabel } from "@/lib/event-style";
import type { ChatSessions, SigninList, Status, UserList } from "@/lib/types";
import { deriveWorkers } from "@/lib/workers";
import { toast } from "sonner";
import type { LucideIcon } from "lucide-react";

// A single at-a-glance monitoring metric.
function Stat({
  icon: Icon, label, value, tint,
}: {
  icon: LucideIcon;
  label: string;
  value: string | number;
  tint: string;
}) {
  return (
    <div className="bg-card flex items-center gap-3 rounded-xl border p-4 shadow-sm">
      <span aria-hidden className={`flex size-9 shrink-0 items-center justify-center rounded-lg ${tint}`}>
        <Icon className="size-4.5" />
      </span>
      <div className="min-w-0">
        <div className="text-xl font-bold tabular-nums">{value}</div>
        <div className="text-muted-foreground text-xs">{label}</div>
      </div>
    </div>
  );
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

  const signinEvents = signins?.available ? (signins.result ?? []).slice(0, 8) : [];
  const workers = deriveWorkers(chat);
  const clients = chat?.clients?.clients ?? [];
  const clientsBySid = new Map(clients.map((c) => [c.sid, c]));

  // Platform-wide monitoring metrics (all users, not the operator).
  const activeSessions = workers.reduce((n, w) => n + (w.ok ? w.sids.length : 0), 0);
  const usersChatting = new Set(clients.map((c) => c.userId)).size;
  const workersOnline = workers.filter((w) => w.ok).length;

  return (
    <div className="space-y-6 px-6 py-6">
      <PageHeader
        title="Sessions"
        description="Live activity across the platform — who's signed in, who's chatting, and worker health."
      />

      {/* At-a-glance monitoring strip (all users) */}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat icon={MessageSquare} label="Active sessions" value={activeSessions}
          tint="bg-blue-500/10 text-blue-600" />
        <Stat icon={UsersIcon} label="Users chatting now" value={usersChatting}
          tint="bg-emerald-500/10 text-emerald-600" />
        <Stat icon={Server} label="Workers online" value={`${workersOnline}/${workers.length}`}
          tint="bg-violet-500/10 text-violet-600" />
        <Stat icon={LogIn} label="Recent sign-ins" value={signinEvents.length}
          tint="bg-amber-500/10 text-amber-700" />
      </div>

      {/* PRIMARY: live chat sessions across every worker — who is chatting now */}
      <Card className="gap-4 p-5">
        <div className="flex items-center gap-2.5">
          <span aria-hidden
            className="flex size-8 items-center justify-center rounded-lg bg-indigo-500/10 text-indigo-600">
            <Server className="size-4" />
          </span>
          <div>
            <h2 className="text-sm font-semibold">Live chat sessions</h2>
            <p className="text-muted-foreground text-xs">
              Every worker, its status, and which user is running each session right now.
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

      {/* Secondary: recent sign-ins (all users) + health + the operator's own
          session demoted to a small card. */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Recent sign-ins — all users (security/audit) */}
        <Card className="gap-4 p-5 lg:col-span-2">
          <h2 className="text-sm font-semibold">Recent sign-ins</h2>
          {signins && signins.available ? (
            signinEvents.length ? (
              <ul className="space-y-2.5">
                {signinEvents.map((e, i) => (
                  <li key={`${e.sequence ?? i}`} className="flex items-center gap-2 text-sm">
                    <span className={eventChipClass(e.type?.type)}>
                      {eventLabel(e.type)}
                    </span>
                    <span className="font-medium">
                      {e.editor?.displayName ?? usersById.get(e.editor?.userId ?? "") ?? "—"}
                    </span>
                    <span className="font-mono text-xs text-muted-foreground">
                      {e.editor?.userId ?? e.aggregate?.id ?? ""}
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

        {/* Platform health */}
        <Card className="gap-3 p-5">
            <h2 className="text-sm font-semibold">Platform health</h2>
            <div className="space-y-3">
              <HealthRow
                ok={status ? (status.health.zitadel ? "ok" : "down") : "warn"}
                label={`Zitadel ${status ? (status.health.zitadel ? "— Operational" : "— Unreachable") : ""}`}
                detail="Identity provider behind every login and admin call."
              />
              <HealthRow
                ok={status?.capabilities.events ? "ok" : "warn"}
                label={`Audit ${status?.capabilities.events ? "on" : "off"}`}
                detail="Event log + sign-in history need IAM_OWNER_VIEWER."
              />
              <HealthRow
                ok={status?.capabilities.chatSessions ? "ok" : "warn"}
                label={`Chat sessions ${status?.capabilities.chatSessions ? "configured" : "off"}`}
                detail="Live panel needs MANAGER_CONTROL_URL on the BFF."
              />
            </div>
        </Card>
      </div>
    </div>
  );
}
