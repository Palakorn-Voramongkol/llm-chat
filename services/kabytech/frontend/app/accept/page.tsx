"use client";
import { useState } from "react";
import { AuthCard, btnCls } from "@/components/Card";
import { Field, PasswordStrength, passwordStrength } from "@/components/Field";

const vPw = (v: string) =>
  v.length < 8
    ? "Password must be at least 8 characters"
    : passwordStrength(v).score < 2
      ? "Add a capital, number, or symbol for a stronger password"
      : undefined;

export default function Page() {
  const [pw, setPw] = useState(""); const [confirm, setConfirm] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [err, setErr] = useState<string | null>(null); const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  // closes over pw so the confirm field re-validates live as the password changes
  const vConfirm = (v: string) => (v !== pw ? "Passwords do not match" : undefined);
  // gate the button: enabled only once the password is strong enough and matches
  const blocked = busy || !!vPw(pw) || !!vConfirm(confirm);

  async function submit(e: React.FormEvent) {
    e.preventDefault(); setErr(null); setSubmitted(true);
    if (vPw(pw) || vConfirm(confirm)) return;
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
    <AuthCard title="Set your password" subtitle="Finish joining KabyTech.">
      <form onSubmit={submit} className="space-y-3" noValidate>
        <div>
          <Field placeholder="Password" type="password" value={pw}
            onChange={setPw} validate={vPw} submitted={submitted} />
          <PasswordStrength value={pw} />
        </div>
        <Field placeholder="Confirm password" type="password" value={confirm}
          onChange={setConfirm} validate={vConfirm} submitted={submitted} />
        {err && <p className="text-sm text-rose-600">{err}</p>}
        <button className={btnCls} disabled={blocked}>{busy ? "Saving…" : "Set password"}</button>
      </form>
    </AuthCard>
  );
}
