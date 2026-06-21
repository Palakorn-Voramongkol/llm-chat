import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { UsageTrend } from "@/components/users/usage-trend";
import type { DailyRow } from "@/lib/types";

describe("UsageTrend", () => {
  const end = new Date("2026-06-21T12:00:00Z");

  it("shows the empty state when no day has activity", () => {
    render(<UsageTrend rows={undefined} endDate={end} />);
    expect(screen.getByText(/no usage in the last 30 days/i)).toBeInTheDocument();
  });

  it("renders a per-day table row for an active day", () => {
    const rows: DailyRow[] = [
      { userId: "u1", day: "2026-06-21", charsIn: 34505, charsOut: 10, fileBytes: 1536 },
    ];
    render(<UsageTrend rows={rows} endDate={end} />);
    expect(screen.getByText("2026-06-21")).toBeInTheDocument();
    expect(screen.getByText("34,505")).toBeInTheDocument();
    expect(screen.getByText("1.5 KB")).toBeInTheDocument();
  });
});
