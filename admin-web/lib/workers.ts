import type { ChatSessions } from "@/lib/types";

// One worker row: derived from the manager's byBackend (the worker's own
// `list` reply — {sessions:[...]} when reachable, {error} when down).
// Shared by the Sessions page and the Dashboard overview.
export interface WorkerRow {
  port: string;
  ok: boolean;
  error?: string;
  sids: string[];
}

export function deriveWorkers(chat: ChatSessions | null): WorkerRow[] {
  const byBackend = (chat?.list?.byBackend ?? {}) as Record<string, unknown>;
  // The instances reply is the authoritative worker list; byBackend keys are
  // the fallback when instances degraded.
  const ports = chat?.instances?.ports?.map(String) ?? Object.keys(byBackend);
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
