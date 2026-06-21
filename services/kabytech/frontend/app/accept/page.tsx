"use client";
import { useState } from "react";
import { AuthCard, inputCls, btnCls } from "@/components/Card";

export default function Page() {
  const [pw, setPw] = useState(""); const [confirm, setConfirm] = useState("");
  const [err, setErr] = useState<string | null>(null); const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setErr(null);
    if (pw.length < 8) return setErr("Password must be at least 8 characters.");
    if (pw !== confirm) return setErr("Passwords do not match.");
    const q = new URLSearchParams(location.search);
    setBusy(true);
    const r = await fetch("/api/accept", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ user_id: q.get("userID"), code: q.get("code"), password: pw }) });
    setBusy(false);
    if (r.ok) setDone(true); else setErr((await r.text()) || "could not set password");
  }

  if (done) return (
    <AuthCard title="You're all set" subtitle="Your password is set. You can sign in now.">
      <a className={btnCls + " block text-center"} href="/login">Go to sign in</a>
    </AuthCard>
  );
  return (
    <AuthCard title="Set your password" subtitle="Finish joining kabytech.">
      <form onSubmit={submit} className="space-y-3">
        <input className={inputCls} type="password" placeholder="Password" value={pw} onChange={(e) => setPw(e.target.value)} />
        <input className={inputCls} type="password" placeholder="Confirm password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={busy}>{busy ? "Saving…" : "Set password"}</button>
      </form>
    </AuthCard>
  );
}
