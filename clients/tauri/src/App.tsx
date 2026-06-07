import { Loader2 } from "lucide-react";
import { AuthProvider, useAuth } from "./auth/useAuth";
import { LoginScreen } from "./auth/LoginScreen";
import { AuthorizationGate } from "./auth/AuthorizationGate";

function SignedInPlaceholder() {
  const { identity, config, signOut } = useAuth();
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
      <h1 className="bg-gradient-to-r from-brand-400 to-brand-600 bg-clip-text text-3xl font-bold text-transparent">
        {config?.app_name}
      </h1>
      <p className="text-sm text-slate-500">
        Signed in as <b>{identity?.email ?? identity?.sub}</b> · roles:{" "}
        {identity?.roles.join(", ") || "none"}
      </p>
      <p className="text-xs text-slate-400">Chat UI lands next.</p>
      <button
        onClick={signOut}
        className="mt-2 rounded-lg border border-slate-300 px-4 py-2 text-sm transition hover:bg-slate-50 dark:border-slate-700 dark:hover:bg-slate-800"
      >
        Sign out
      </button>
    </div>
  );
}

function Shell() {
  const { loading, identity } = useAuth();
  if (loading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="animate-spin text-brand-500" size={28} />
      </div>
    );
  }
  if (!identity) return <LoginScreen />;
  return (
    <AuthorizationGate>
      <SignedInPlaceholder />
    </AuthorizationGate>
  );
}

export default function App() {
  return (
    <AuthProvider>
      <Shell />
    </AuthProvider>
  );
}
