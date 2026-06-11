"use client";
import { useCallback, useEffect, useState } from "react";
import { Bell } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { api } from "@/lib/api";
import { eventLabel, eventSeverity, isSignificantEvent } from "@/lib/event-style";
import type { AuditEvent, Capabilities, EventList } from "@/lib/types";

const SEVERITY_DOT: Record<string, string> = {
  success: "bg-success",
  warning: "bg-warning",
  danger: "bg-danger",
  info: "bg-info",
};

// Read-state lives in localStorage as the millisecond timestamp of the newest
// event the operator has seen. Anything newer is "unread". Cross-device unread
// state / real-time push would need a backend notifications service — this is
// the client-side notification center over the existing audit feed.
const SEEN_KEY = "console.notif.lastSeen";

function readLastSeen(): number {
  if (typeof window === "undefined") return 0;
  const v = window.localStorage.getItem(SEEN_KEY);
  return v ? Number(v) || 0 : 0;
}

const toMs = (e: AuditEvent) => (e.creationDate ? new Date(e.creationDate).getTime() : 0);

export function NotificationBell() {
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [available, setAvailable] = useState(true);
  const [lastSeen, setLastSeen] = useState<number>(() => readLastSeen());

  const load = useCallback(async () => {
    try {
      const caps = await api.get<Capabilities>("/api/capabilities");
      if (!caps.events) {
        setAvailable(false);
        return;
      }
      const list = await api.get<EventList>("/api/events");
      // Curate: only administratively significant events reach the bell
      // (no token/session churn). The Audit page still shows the full log.
      setEvents(
        (list.result ?? [])
          .filter((e) => isSignificantEvent(e.type?.type))
          .slice(0, 12),
      );
    } catch {
      // Swallow — the bell simply shows nothing rather than erroring the chrome.
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const unread = events.filter((e) => toMs(e) > lastSeen).length;

  // Mark everything seen when the panel CLOSES (so unread items stay highlighted
  // while it's open), persisting the newest timestamp.
  const markRead = () => {
    const newest = events.reduce((m, e) => Math.max(m, toMs(e)), lastSeen);
    setLastSeen(newest);
    try {
      window.localStorage.setItem(SEEN_KEY, String(newest));
    } catch {
      /* ignore quota / disabled storage */
    }
  };

  return (
    <DropdownMenu onOpenChange={(open) => !open && markRead()}>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          className="relative size-8 p-0"
          aria-label={`Notifications${unread ? ` (${unread} unread)` : ""}`}
        >
          <Bell className="size-4" />
          {unread > 0 && (
            <Badge
              variant="destructive"
              className="absolute -top-1 -right-1 min-w-4 justify-center rounded-full px-1 py-0 text-[10px] tabular-nums"
            >
              {unread > 9 ? "9+" : unread}
            </Badge>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-80 p-0">
        <div className="flex items-center justify-between border-b px-3 py-2">
          <span className="text-sm font-semibold">Notifications</span>
          {available && events.length > 0 && (
            <span className="text-muted-foreground text-xs">
              {unread ? `${unread} new` : "all caught up"}
            </span>
          )}
        </div>
        <div className="max-h-96 overflow-auto">
          {!available ? (
            <p className="text-muted-foreground p-4 text-sm">
              Notifications need the audit capability (IAM_OWNER_VIEWER on the
              service account).
            </p>
          ) : events.length === 0 ? (
            <p className="text-muted-foreground p-4 text-sm">No recent activity.</p>
          ) : (
            <ul className="py-1">
              {events.map((e, i) => {
                const fresh = toMs(e) > lastSeen;
                const who =
                  e.editor?.displayName ?? e.editor?.service ?? e.aggregate?.id ?? "—";
                const when = e.creationDate
                  ? new Date(e.creationDate).toLocaleString()
                  : "";
                // Uniform two-line layout: a fixed status dot + a title line and
                // a muted detail line, both truncated so every row is the same
                // width. Theme tokens only (works in light + dark).
                return (
                  <li
                    key={e.sequence ?? i}
                    className={`flex items-start gap-2.5 px-3 py-2 ${fresh ? "bg-accent" : ""}`}
                  >
                    <span
                      aria-hidden
                      className={`mt-1.5 size-2 shrink-0 rounded-full ${SEVERITY_DOT[eventSeverity(e.type?.type)]}`}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium">{eventLabel(e.type)}</div>
                      <div className="text-muted-foreground truncate text-xs">
                        {who}
                        {when ? ` · ${when}` : ""}
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
