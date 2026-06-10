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
