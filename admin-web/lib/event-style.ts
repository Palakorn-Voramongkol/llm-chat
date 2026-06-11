// Tinted chip classes per Zitadel event category (shared by the Audit table
// and the Sessions recent-sign-ins list). Keyed on the raw event type string.
// rounded-md, not rounded-full: these chips often carry long dotted type names
// and a pill shape reads badly on long text.
export function eventChipClass(type: string | undefined): string {
  const base =
    "inline-flex items-center rounded-md px-2 py-0.5 text-xs font-medium";
  if (!type) return `${base} bg-slate-500/10 text-slate-600`;
  if (type.startsWith("user.")) return `${base} bg-blue-500/10 text-blue-600`;
  if (type.startsWith("oidc_session.")) return `${base} bg-violet-500/10 text-violet-700`;
  if (type.startsWith("project.")) return `${base} bg-emerald-500/10 text-emerald-700`;
  if (type.startsWith("org.") || type.includes("policy"))
    return `${base} bg-amber-500/10 text-amber-700`;
  return `${base} bg-slate-500/10 text-slate-600`;
}

// --- Notification significance (the bell shows only what an admin must know) ---
//
// Zitadel emits a flood of low-signal events — token issuance, session refresh,
// login password checks. Those are NOISE for a notification center. We surface
// only administrative CHANGES and security-relevant lifecycle: users added/
// removed/locked, grant/role changes, app changes, secret/key changes, project
// and org/policy changes. Everything else (incl. token.*/oidc_session.*) is
// hidden from the bell (the Audit page still shows the full log).

const NOISE_PATTERNS = [
  "oidc_session.", "user.token.", ".token.", "session.",
  ".check.", ".search", "instance.", "notification.", "user.human.init",
];

const SIGNIFICANT_PATTERNS = [
  // user lifecycle + security
  "user.added", "user.selfregistered", "user.removed",
  "user.deactivated", "user.reactivated", "user.locked", "user.unlocked",
  "user.human.password.changed", "user.machine.key", "user.machine.secret",
  // authorization
  "user.grant.", "project.role.",
  // applications
  "project.application", "project.added", "project.removed", "project.changed",
  // org / policy
  "org.added", "org.removed", "org.deactivated", "org.changed", "policy",
];

/** True if an event is worth showing in the notification bell. */
export function isSignificantEvent(type: string | undefined): boolean {
  if (!type) return false;
  if (NOISE_PATTERNS.some((p) => type.includes(p))) return false;
  return SIGNIFICANT_PATTERNS.some((p) => type.startsWith(p) || type.includes(p));
}

/** Semantic severity for a notification dot/accent. */
export function eventSeverity(
  type: string | undefined,
): "success" | "warning" | "danger" | "info" {
  if (!type) return "info";
  if (/(removed|deleted|deactivated|locked)/.test(type)) return "danger";
  if (/(added|created|reactivated|unlocked|selfregistered)/.test(type)) return "success";
  if (/(changed|updated|password|secret|key|grant|role|policy)/.test(type)) return "warning";
  return "info";
}

// Display label for an event: prefer the localized message; otherwise show the
// raw type WITHOUT the noisy "EventTypes." prefix Zitadel sometimes returns in
// localizedMessage for untranslated types.
export function eventLabel(
  type: { type?: string; localized?: { localizedMessage?: string } } | undefined,
): string {
  const localized = type?.localized?.localizedMessage;
  const raw = type?.type;
  const label = localized ?? raw ?? "—";
  return label.startsWith("EventTypes.") ? label.slice("EventTypes.".length) : label;
}
