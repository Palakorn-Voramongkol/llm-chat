/** Build the chat-sessions API URL, scoped to a selected application key.
 * Empty key → the backend's default app (no `?app=`). */
export function chatSessionsUrl(app: string): string {
  return app ? `/api/chat-sessions?app=${encodeURIComponent(app)}` : "/api/chat-sessions";
}
