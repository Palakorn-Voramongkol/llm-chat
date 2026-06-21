"use client";
import { useState } from "react";
import { AuthCard, inputCls, btnCls } from "@/components/Card";

export default function Page() {
  const [email, setEmail] = useState(""); const [given, setGiven] = useState("");
  const [family, setFamily] = useState(""); const [sent, setSent] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null); const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setBusy(true); setErr(null);
    const r = await fetch("/api/invite", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email, given, family }) });
    setBusy(false);
    if (r.ok) setSent(email); else setErr((await r.text()) || "invite failed");
  }

  if (sent) return (
    <AuthCard title="Invite sent" subtitle={`An invitation email is on its way to ${sent}.`}>
      <button className={btnCls} onClick={() => { setSent(null); setEmail(""); }}>Invite another</button>
    </AuthCard>
  );
  return (
    <AuthCard title="Invite a user" subtitle="They'll get an email to set their password and join.">
      <form onSubmit={submit} className="space-y-3">
        <input className={inputCls} placeholder="Email address" type="email" required
          value={email} onChange={(e) => setEmail(e.target.value)} />
        <div className="flex gap-3">
          <input className={inputCls} placeholder="First name" value={given} onChange={(e) => setGiven(e.target.value)} />
          <input className={inputCls} placeholder="Last name" value={family} onChange={(e) => setFamily(e.target.value)} />
        </div>
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={busy}>{busy ? "Sending…" : "Send invite"}</button>
      </form>
    </AuthCard>
  );
}
