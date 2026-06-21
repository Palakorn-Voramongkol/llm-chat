import { describe, it, expect } from "vitest";
import { buildDailySeries } from "@/lib/usage-daily";
import type { DailyRow } from "@/lib/types";

describe("buildDailySeries", () => {
  const end = new Date("2026-06-21T12:00:00Z");

  it("returns 30 entries, oldest first, ending on endDate", () => {
    const s = buildDailySeries([], end);
    expect(s).toHaveLength(30);
    expect(s[0].day).toBe("2026-05-23");
    expect(s[29].day).toBe("2026-06-21");
  });

  it("zero-fills missing days and places rows on the right day", () => {
    const rows: DailyRow[] = [
      { userId: "u1", day: "2026-06-21", tokensIn: 130, tokensOut: 5, costUsd: 0.5 },
      { userId: "u1", day: "2026-06-19", tokensIn: 40, tokensOut: 2, costUsd: 0.1 },
    ];
    const s = buildDailySeries(rows, end);
    const byDay = Object.fromEntries(s.map((d) => [d.day, d]));
    expect(byDay["2026-06-21"].tokensIn).toBe(130);
    expect(byDay["2026-06-19"].tokensOut).toBe(2);
    expect(byDay["2026-06-20"]).toEqual({ day: "2026-06-20", tokensIn: 0, tokensOut: 0, costUsd: 0 });
  });

  it("undefined rows -> all zeros", () => {
    const s = buildDailySeries(undefined, end);
    expect(s.every((d) => d.tokensIn === 0 && d.tokensOut === 0 && d.costUsd === 0)).toBe(true);
  });
});
