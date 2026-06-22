"use client";
import { useState } from "react";
import { inputCls } from "@/components/Card";

const inputErrCls =
  "w-full rounded-lg border border-rose-400 bg-rose-50/40 px-3.5 py-2.5 text-sm text-slate-900 outline-none transition placeholder:text-slate-400 focus:border-rose-500 focus:ring-2 focus:ring-rose-500/30";

/** PURE: a forgiving email check (one @, a dot in the domain). */
export function isEmail(s: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(s.trim());
}

/** PURE: score a password 0–4 (length + character variety) with a label. */
export function passwordStrength(pw: string): { score: number; label: string } {
  let s = 0;
  if (pw.length >= 8) s++;
  if (pw.length >= 12) s++;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) s++;
  if (/\d/.test(pw)) s++;
  if (/[^A-Za-z0-9]/.test(pw)) s++;
  const score = Math.min(s, 4);
  const label = ["Too weak", "Weak", "Fair", "Good", "Strong"][score];
  return { score, label };
}

/** Real-time password strength meter (4 segments + label). Renders nothing for
 * an empty value. */
export function PasswordStrength({ value }: { value: string }) {
  if (!value) return null;
  const { score, label } = passwordStrength(value);
  const fill = ["bg-rose-500", "bg-orange-500", "bg-amber-500", "bg-lime-500", "bg-emerald-500"][score];
  const text = ["text-rose-600", "text-orange-600", "text-amber-600", "text-lime-600", "text-emerald-600"][score];
  return (
    <div className="mt-2" aria-live="polite">
      <div className="flex gap-1">
        {[0, 1, 2, 3].map((i) => (
          <div key={i} className={`h-1 flex-1 rounded-full ${i < score ? fill : "bg-slate-200"}`} />
        ))}
      </div>
      <p className="mt-1 text-xs text-slate-500">
        Password strength: <span className={`font-medium ${text}`}>{label}</span>
      </p>
    </div>
  );
}

/** A labelled input with REAL-TIME inline validation. `validate(value)` returns
 * an error message (or undefined). The error shows live — as soon as the field
 * is touched (first keystroke or blur) and updates on every change — plus
 * whenever `submitted` is set (so a submit reveals untouched-field errors). */
export function Field({
  placeholder, type = "text", value, onChange, validate, autoComplete, submitted,
}: {
  placeholder: string;
  type?: string;
  value: string;
  onChange: (v: string) => void;
  validate?: (v: string) => string | undefined;
  autoComplete?: string;
  submitted?: boolean;
}) {
  const [touched, setTouched] = useState(false);
  const error = (touched || submitted) ? validate?.(value) : undefined;
  return (
    <div>
      <input
        className={error ? inputErrCls : inputCls}
        type={type}
        placeholder={placeholder}
        autoComplete={autoComplete}
        aria-invalid={error ? true : undefined}
        value={value}
        onChange={(e) => { if (!touched) setTouched(true); onChange(e.target.value); }}
        onBlur={() => setTouched(true)}
      />
      {error && <p className="mt-1 text-xs text-rose-600">{error}</p>}
    </div>
  );
}
