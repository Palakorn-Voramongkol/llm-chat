import { type ReactNode } from "react";
import { LogOut, ShieldAlert } from "lucide-react";
import { useAuth } from "./useAuth";

/**
 * Authorization beyond authentication: render children only if the signed-in
 * account holds the required app role. Otherwise show a clear access-denied
 * screen (the chat UI never mounts).
 */
export function AuthorizationGate({ children }: { children: ReactNode }) {
  const { identity, config, signOut } = useAuth();
  if (!identity) return null;

  const required = config?.required_role ?? "chat.app";
  if (identity.roles.includes(required)) return <>{children}</>;

  return (
    <div className="flex h-full items-center justify-center p-6">
      <div className="w-[440px] animate-fade-in rounded-2xl border border-slate-200 bg-white p-8 text-center shadow-2xl dark:border-slate-800 dark:bg-slate-900">
        <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-amber-100 text-amber-600 dark:bg-amber-950/50 dark:text-amber-400">
          <ShieldAlert size={28} />
        </div>
        <h2 className="text-xl font-semibold">
          You don't have access to {config?.app_name ?? "this app"}
        </h2>
        <p className="mt-2 text-sm leading-relaxed text-slate-500">
          You're signed in as <b>{identity.email ?? identity.sub}</b>, but your account is missing
          the{" "}
          <code className="rounded bg-slate-100 px-1.5 py-0.5 font-mono text-[0.85em] dark:bg-slate-800">
            {required}
          </code>{" "}
          role. Ask an administrator to grant it, then sign in again.
        </p>
        <button
          onClick={signOut}
          className="mx-auto mt-6 flex items-center gap-2 rounded-lg border border-slate-300 px-4 py-2 text-sm transition hover:bg-slate-50 dark:border-slate-700 dark:hover:bg-slate-800"
        >
          <LogOut size={16} /> Sign out
        </button>
      </div>
    </div>
  );
}
