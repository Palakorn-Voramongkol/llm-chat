// Day separator shown before the first message of each calendar day — a
// horizontal rule with the date centered (LINE/WhatsApp style). The grouping
// logic in ChatView calls dayKey() to detect a day change and dayLabel() for
// the text.

/// Local calendar-day key (year-month-day) used to detect when a new day starts.
export function dayKey(t: number): string {
  const d = new Date(t);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

/// Friendly day label: "Today" / "Yesterday" / a full weekday-date otherwise.
export function dayLabel(t: number): string {
  const d = new Date(t);
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diffDays = Math.round((startOf(new Date()) - startOf(d)) / 86_400_000);
  if (diffDays === 0) return "Today";
  if (diffDays === 1) return "Yesterday";
  return d.toLocaleDateString([], {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

export function DayDivider({ label }: { label: string }) {
  return (
    <div className="my-1 flex select-none items-center gap-3" aria-label={label}>
      <div className="h-px flex-1 bg-slate-200 dark:bg-slate-800" />
      <span className="shrink-0 text-[11px] font-medium uppercase tracking-wide text-slate-400">
        {label}
      </span>
      <div className="h-px flex-1 bg-slate-200 dark:bg-slate-800" />
    </div>
  );
}
