import type { DailyRow } from "@/lib/types";

export interface DaySeries {
  day: string;
  charsIn: number;
  charsOut: number;
  fileBytes: number;
}

/** UTC "YYYY-MM-DD" for a Date. */
function dayKey(d: Date): string {
  return d.toISOString().slice(0, 10);
}

/**
 * The trailing 30 days ending on `endDate` (inclusive), oldest -> newest, with
 * each `rows` entry placed on its `day` and every other day zero-filled. Pure:
 * `endDate` is passed in so the result is deterministic and unit-testable.
 */
export function buildDailySeries(rows: DailyRow[] | undefined, endDate: Date): DaySeries[] {
  const byDay = new Map<string, DailyRow>();
  for (const r of rows ?? []) byDay.set(r.day, r);
  const out: DaySeries[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date(endDate);
    d.setUTCDate(d.getUTCDate() - i);
    const key = dayKey(d);
    const r = byDay.get(key);
    out.push({
      day: key,
      charsIn: r?.charsIn ?? 0,
      charsOut: r?.charsOut ?? 0,
      fileBytes: r?.fileBytes ?? 0,
    });
  }
  return out;
}
