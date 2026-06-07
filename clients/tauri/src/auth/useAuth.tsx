import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { api, type AppConfig, type Identity } from "../lib/tauri";

interface AuthState {
  config: AppConfig | null;
  identity: Identity | null;
  loading: boolean; // initial restore in flight
  signingIn: boolean;
  error: string | null;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
}

const Ctx = createContext<AuthState | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [loading, setLoading] = useState(true);
  const [signingIn, setSigningIn] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        setConfig(await api.getConfig());
        const id = await api.restore();
        if (id) setIdentity(id);
      } catch {
        // ignore restore failures — fall through to the login screen
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const signIn = async () => {
    setSigningIn(true);
    setError(null);
    try {
      setIdentity(await api.login());
    } catch (e) {
      setError(String(e));
    } finally {
      setSigningIn(false);
    }
  };

  const signOut = async () => {
    try {
      await api.logout();
    } finally {
      setIdentity(null);
    }
  };

  return (
    <Ctx.Provider value={{ config, identity, loading, signingIn, error, signIn, signOut }}>
      {children}
    </Ctx.Provider>
  );
}

export function useAuth(): AuthState {
  const c = useContext(Ctx);
  if (!c) throw new Error("useAuth must be used within <AuthProvider>");
  return c;
}
