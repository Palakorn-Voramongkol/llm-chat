import { useEffect, useRef } from "react";
import { LogOut, Sparkles } from "lucide-react";
import { useAuth } from "../auth/useAuth";
import { useChat } from "./useChat";
import { Message } from "./Message";
import { Composer } from "./Composer";

export function ChatView() {
  const { config, identity, signOut } = useAuth();
  const { messages, connected, thinking, connError, send } = useChat();
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const plantuml = config?.plantuml_server ?? "https://www.plantuml.com/plantuml";
  const name = config?.app_name ?? "Lumina";

  return (
    <div className="flex h-full flex-col bg-slate-50 dark:bg-slate-950">
      <header className="flex items-center justify-between border-b border-slate-200 bg-white/70 px-4 py-2.5 backdrop-blur dark:border-slate-800 dark:bg-slate-900/60">
        <div className="flex items-center gap-2">
          <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-gradient-to-br from-brand-400 to-brand-600 text-white">
            <Sparkles size={15} />
          </div>
          <span className="font-semibold">{name}</span>
          <span
            className={`ml-1 h-2 w-2 rounded-full ${connected ? "bg-emerald-500" : "bg-amber-400"}`}
            title={connected ? "connected" : connError ?? "connecting…"}
          />
        </div>
        <div className="flex items-center gap-3 text-sm text-slate-500">
          <span className="hidden sm:inline">{identity?.email ?? identity?.sub}</span>
          <button
            onClick={signOut}
            title="Sign out"
            className="flex items-center gap-1 rounded-lg px-2 py-1 transition hover:bg-slate-100 dark:hover:bg-slate-800"
          >
            <LogOut size={15} />
          </button>
        </div>
      </header>

      <div className="flex-1 overflow-y-auto">
        <div className="flex w-full flex-col gap-5 px-4 py-5 sm:px-6 lg:px-10 xl:px-16">
          {connError && (
            <div className="rounded-lg bg-red-50 px-3 py-2 text-center text-sm text-red-600 dark:bg-red-950/40 dark:text-red-400">
              {connError}
            </div>
          )}
          {messages.length === 0 && !connError && (
            <div className="mt-20 text-center text-slate-400">
              <Sparkles className="mx-auto mb-3 text-brand-400" size={32} />
              <p className="text-lg font-medium text-slate-600 dark:text-slate-300">Ask {name} anything</p>
              <p className="mt-1 text-sm">
                Answers render with code, math, diagrams, tables, HTML and PDFs.
              </p>
            </div>
          )}
          {messages.map((m) => (
            <div key={m.id} className="animate-fade-in">
              <Message msg={m} plantumlServer={plantuml} />
            </div>
          ))}
          <div ref={endRef} />
        </div>
      </div>

      <Composer onSend={send} disabled={thinking} />
    </div>
  );
}
