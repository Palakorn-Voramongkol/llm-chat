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
      { userId: "u1", day: "2026-06-21", charsIn: 130, charsOut: 5, fileBytes: 2048 },
      { userId: "u1", day: "2026-06-19", charsIn: 40, charsOut: 2, fileBytes: 0 },
    ];
    const s = buildDailySeries(rows, end);
    const byDay = Object.fromEntries(s.map((d) => [d.day, d]));
    expect(byDay["2026-06-21"].charsIn).toBe(130);
    expect(byDay["2026-06-21"].fileBytes).toBe(2048);
    expect(byDay["2026-06-19"].charsOut).toBe(2);
    expect(byDay["2026-06-20"]).toEqual({ day: "2026-06-20", charsIn: 0, charsOut: 0, fileBytes: 0 });
  });

  it("undefined rows -> all zeros", () => {
    const s = buildDailySeries(undefined, end);
    expect(s.every((d) => d.charsIn === 0 && d.charsOut === 0 && d.fileBytes === 0)).toBe(true);
  });
});
