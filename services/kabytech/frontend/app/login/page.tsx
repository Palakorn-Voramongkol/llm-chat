"use client";
import { useEffect, useState } from "react";
import { AuthCard, inputCls, btnCls } from "@/components/Card";

export default function Page() {
  const [authRequest, setAuthRequest] = useState<string | null>(null);
  const [loginName, setLoginName] = useState(""); const [password, setPassword] = useState("");
  const [err, setErr] = useState<string | null>(null); const [busy, setBusy] = useState(false);

  useEffect(() => {
    const ar = new URLSearchParams(location.search).get("authRequest");
    if (ar) setAuthRequest(ar);
    else location.href = "/api/login/start"; // begin the OIDC flow (Zitadel bounces back here)
  }, []);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setErr(null); setBusy(true);
    const r = await fetch("/api/login", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ auth_request: authRequest, login_name: loginName, password }) });
    setBusy(false);
    if (r.ok) { const { callbackUrl } = await r.json(); location.href = callbackUrl; }
    else setErr((await r.text()) || "sign in failed");
  }

  if (!authRequest) return <AuthCard title="Signing in…"><p className="text-slate-500">Redirecting…</p></AuthCard>;
  return (
    <AuthCard title="Sign in to kabytech" subtitle="Welcome back.">
      <form onSubmit={submit} className="space-y-3">
        <input className={inputCls} placeholder="Email or username" autoComplete="username"
          value={loginName} onChange={(e) => setLoginName(e.target.value)} />
        <input className={inputCls} type="password" placeholder="Password" autoComplete="current-password"
          value={password} onChange={(e) => setPassword(e.target.value)} />
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button>
      </form>
    </AuthCard>
  );
}
