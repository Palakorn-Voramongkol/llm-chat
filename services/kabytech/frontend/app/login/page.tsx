"use client";
import { useEffect, useState } from "react";
import { AuthCard, btnCls } from "@/components/Card";
import { Field, PasswordStrength } from "@/components/Field";

const vLogin = (v: string) => (!v.trim() ? "Enter your email or username" : undefined);
const vPassword = (v: string) => (!v ? "Enter your password" : undefined);

export default function Page() {
  const [authRequest, setAuthRequest] = useState<string | null>(null);
  const [loginName, setLoginName] = useState(""); const [password, setPassword] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [err, setErr] = useState<string | null>(null); const [busy, setBusy] = useState(false);

  useEffect(() => {
    const ar = new URLSearchParams(location.search).get("authRequest");
    if (ar) setAuthRequest(ar);
    else location.href = "/api/login/start"; // begin the OIDC flow (Zitadel bounces back here)
  }, []);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setErr(null); setSubmitted(true);
    if (vLogin(loginName) || vPassword(password)) return;
    setBusy(true);
    const r = await fetch("/api/login", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ auth_request: authRequest, login_name: loginName, password }) });
    setBusy(false);
    if (r.ok) { const { callbackUrl } = await r.json(); location.href = callbackUrl; }
    else setErr((await r.text()) || "sign in failed");
  }

  if (!authRequest) return <AuthCard title="Signing in…"><p className="text-slate-500">Redirecting…</p></AuthCard>;
  return (
    <AuthCard title="Sign in to KabyTech" subtitle="Welcome back.">
      <form onSubmit={submit} className="space-y-3" noValidate>
        <Field placeholder="Email or username" autoComplete="username" value={loginName}
          onChange={setLoginName} validate={vLogin} submitted={submitted} />
        <div>
          <Field placeholder="Password" type="password" autoComplete="current-password" value={password}
            onChange={setPassword} validate={vPassword} submitted={submitted} />
          <PasswordStrength value={password} />
        </div>
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={busy}>{busy ? "Signing in…" : "Sign in"}</button>
      </form>
    </AuthCard>
  );
}
