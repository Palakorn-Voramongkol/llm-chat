import { Loader2, Sparkles, User } from "lucide-react";
import { Markdown } from "../render/Markdown";
import type { ChatMessage } from "./useChat";

export function Message({ msg, plantumlServer }: { msg: ChatMessage; plantumlServer: string }) {
  const isUser = msg.role === "user";
  return (
    <div className={`flex gap-3 ${isUser ? "flex-row-reverse" : ""}`}>
      <div
        className={`mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full ${
          isUser
            ? "bg-slate-200 text-slate-600 dark:bg-slate-700 dark:text-slate-200"
            : "bg-gradient-to-br from-brand-400 to-brand-600 text-white"
        }`}
      >
        {isUser ? <User size={16} /> : <Sparkles size={16} />}
      </div>
      <div
        className={`min-w-0 rounded-2xl px-4 py-2.5 ${
          isUser
            ? "max-w-[80%] bg-brand-600 text-white"
            : "max-w-[94%] border border-slate-200 bg-white shadow-sm dark:border-slate-800 dark:bg-slate-900"
        } ${msg.error ? "ring-1 ring-red-400" : ""}`}
      >
        {isUser ? (
          <p className="whitespace-pre-wrap">{msg.text}</p>
        ) : msg.pending ? (
          <span className="flex items-center gap-2 text-slate-500">
            <Loader2 className="animate-spin" size={15} /> thinking…
          </span>
        ) : (
          <Markdown content={msg.text} plantumlServer={plantumlServer} />
        )}
        {!isUser && !msg.pending && msg.latencyMs != null && msg.latencyMs >= 0 && (
          <div className="mt-1 text-[11px] text-slate-400">{(msg.latencyMs / 1000).toFixed(1)}s</div>
        )}
      </div>
    </div>
  );
}
