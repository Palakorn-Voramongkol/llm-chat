import { LogIn, Loader2, Sparkles } from "lucide-react";
import { useAuth } from "./useAuth";

function issuerHost(issuer?: string): string {
  if (!issuer) return "your identity provider";
  try {
    return new URL(issuer).host;
  } catch {
    return issuer;
  }
}

export function LoginScreen() {
  const { config, signIn, signingIn, error } = useAuth();
  const name = config?.app_name ?? "Lumina";

  return (
    <div className="flex h-full items-center justify-center bg-gradient-to-br from-slate-100 to-brand-50 dark:from-slate-950 dark:to-slate-900">
      <div className="w-[380px] animate-fade-in rounded-2xl border border-slate-200 bg-white/85 p-8 shadow-2xl backdrop-blur dark:border-slate-800 dark:bg-slate-900/70">
        <div className="mb-7 flex flex-col items-center text-center">
          <div className="mb-3 flex h-14 w-14 items-center justify-center rounded-2xl bg-gradient-to-br from-brand-400 to-brand-600 text-white shadow-lg">
            <Sparkles size={28} />
          </div>
          <h1 className="text-2xl font-bold tracking-tight">{name}</h1>
          <p className="mt-1 text-sm text-slate-500">Rich answers, beautifully rendered.</p>
        </div>

        <button
          onClick={signIn}
          disabled={signingIn}
          className="flex w-full items-center justify-center gap-2 rounded-xl bg-brand-600 px-4 py-2.5 font-medium text-white shadow-sm transition hover:bg-brand-500 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {signingIn ? (
            <>
              <Loader2 className="animate-spin" size={18} /> Waiting for sign-in…
            </>
          ) : (
            <>
              <LogIn size={18} /> Sign in
            </>
          )}
        </button>

        {signingIn && (
          <p className="mt-3 text-center text-xs text-slate-500">
            Complete the sign-in in your browser, then return here.
          </p>
        )}
        {error && (
          <p className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-center text-xs text-red-600 dark:bg-red-950/40 dark:text-red-400">
            {error}
          </p>
        )}

        <p className="mt-7 text-center text-[11px] text-slate-400">
          Secure sign-in via {issuerHost(config?.issuer)}
        </p>
      </div>
    </div>
  );
}
