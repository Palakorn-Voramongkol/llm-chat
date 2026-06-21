"use client";
import {
  CartesianGrid, ComposedChart, Line, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from "recharts";
import type { DailyRow } from "@/lib/types";
import { buildDailySeries } from "@/lib/usage-daily";
import { fmtCount, fmtBytes } from "@/components/users/columns";

/** A user's last-30-day self-counted trend: a dual-axis line chart (chars in/out
 * left, attachment bytes right) over the zero-filled 30-day window, plus a
 * per-day breakdown table. `rows` is the user's daily buckets (may be undefined
 * when the fetch failed or the user has no usage); `endDate` is "today" (passed
 * in for testability). These are the platform's OWN counts, not claude tokens. */
export function UsageTrend({ rows, endDate }: { rows: DailyRow[] | undefined; endDate: Date }) {
  const series = buildDailySeries(rows, endDate);
  const active = series.filter((d) => d.charsIn || d.charsOut || d.fileBytes);
  if (active.length === 0) {
    return <p className="text-muted-foreground text-sm">No usage in the last 30 days.</p>;
  }
  const mmdd = (day: string) => day.slice(5); // "MM-DD"
  // Compact char-axis labels so 5-digit counts don't clip: 34564 -> "35k".
  const kfmt = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : `${n}`);
  // Compact byte-axis labels: 1048576 -> "1.0 MB".
  const bfmt = (n: number) =>
    n < 1024 ? `${n}` : n < 1024 * 1024 ? `${Math.round(n / 1024)}K` : `${(n / 1048576).toFixed(1)}M`;
  return (
    <div className="space-y-3">
      <ResponsiveContainer width="100%" height={200}>
        <ComposedChart data={series} margin={{ top: 4, right: 8, left: 4, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" vertical={false} />
          <XAxis dataKey="day" tickFormatter={mmdd} tick={{ fontSize: 11 }}
            tickLine={false} axisLine={false} minTickGap={24} />
          <YAxis yAxisId="chars" tickFormatter={kfmt} tick={{ fontSize: 11 }}
            tickLine={false} axisLine={false} width={44} />
          <YAxis yAxisId="bytes" orientation="right" tick={{ fontSize: 11 }} tickLine={false}
            axisLine={false} width={48} tickFormatter={bfmt} />
          <Tooltip
            formatter={(value, name) =>
              name === "File bytes" ? fmtBytes(Number(value)) : fmtCount(Number(value))}
            labelFormatter={(label) => String(label)} />
          <Line yAxisId="chars" type="monotone" dataKey="charsIn" name="Chars in"
            stroke="#5b53e8" dot={false} strokeWidth={2} />
          <Line yAxisId="chars" type="monotone" dataKey="charsOut" name="Chars out"
            stroke="#10b981" dot={false} strokeWidth={2} />
          <Line yAxisId="bytes" type="monotone" dataKey="fileBytes" name="File bytes"
            stroke="#f59e0b" dot={false} strokeWidth={2} />
        </ComposedChart>
      </ResponsiveContainer>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-muted-foreground text-left">
            <th className="font-medium">Day</th>
            <th className="font-medium text-right">Chars in</th>
            <th className="font-medium text-right">Chars out</th>
            <th className="font-medium text-right">File bytes</th>
          </tr>
        </thead>
        <tbody>
          {[...active].reverse().map((d) => (
            <tr key={d.day}>
              <td>{d.day}</td>
              <td className="text-right tabular-nums">{fmtCount(d.charsIn)}</td>
              <td className="text-right tabular-nums">{fmtCount(d.charsOut)}</td>
              <td className="text-right tabular-nums">{fmtBytes(d.fileBytes)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
