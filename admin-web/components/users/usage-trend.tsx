"use client";
import {
  CartesianGrid, ComposedChart, Line, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from "recharts";
import type { DailyRow } from "@/lib/types";
import { buildDailySeries } from "@/lib/usage-daily";
import { fmtTokens, fmtCost } from "@/components/users/columns";

/** A user's last-30-day token trend: a dual-axis line chart (tokens left, cost
 * right) over the zero-filled 30-day window, plus a per-day breakdown table.
 * `rows` is the user's daily buckets (may be undefined when the fetch failed or
 * the user has no usage); `endDate` is "today" (passed in for testability). */
export function UsageTrend({ rows, endDate }: { rows: DailyRow[] | undefined; endDate: Date }) {
  const series = buildDailySeries(rows, endDate);
  const active = series.filter((d) => d.tokensIn || d.tokensOut || d.costUsd);
  if (active.length === 0) {
    return <p className="text-muted-foreground text-sm">No usage in the last 30 days.</p>;
  }
  const mmdd = (day: string) => day.slice(5); // "MM-DD"
  // Compact token-axis labels so 5-digit counts don't clip: 34564 -> "35k".
  const kfmt = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : `${n}`);
  return (
    <div className="space-y-3">
      <ResponsiveContainer width="100%" height={200}>
        <ComposedChart data={series} margin={{ top: 4, right: 8, left: 4, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" vertical={false} />
          <XAxis dataKey="day" tickFormatter={mmdd} tick={{ fontSize: 11 }}
            tickLine={false} axisLine={false} minTickGap={24} />
          <YAxis yAxisId="tok" tickFormatter={kfmt} tick={{ fontSize: 11 }}
            tickLine={false} axisLine={false} width={44} />
          <YAxis yAxisId="cost" orientation="right" tick={{ fontSize: 11 }} tickLine={false}
            axisLine={false} width={48} tickFormatter={(v: number) => `$${v.toFixed(2)}`} />
          <Tooltip
            formatter={(value, name) =>
              name === "Cost" ? fmtCost(Number(value)) : fmtTokens(Number(value))}
            labelFormatter={(label) => String(label)} />
          <Line yAxisId="tok" type="monotone" dataKey="tokensIn" name="Tokens in"
            stroke="#5b53e8" dot={false} strokeWidth={2} />
          <Line yAxisId="tok" type="monotone" dataKey="tokensOut" name="Tokens out"
            stroke="#10b981" dot={false} strokeWidth={2} />
          <Line yAxisId="cost" type="monotone" dataKey="costUsd" name="Cost"
            stroke="#f59e0b" dot={false} strokeWidth={2} />
        </ComposedChart>
      </ResponsiveContainer>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-muted-foreground text-left">
            <th className="font-medium">Day</th>
            <th className="font-medium text-right">Tokens in</th>
            <th className="font-medium text-right">Tokens out</th>
            <th className="font-medium text-right">Cost</th>
          </tr>
        </thead>
        <tbody>
          {[...active].reverse().map((d) => (
            <tr key={d.day}>
              <td>{d.day}</td>
              <td className="text-right tabular-nums">{fmtTokens(d.tokensIn)}</td>
              <td className="text-right tabular-nums">{fmtTokens(d.tokensOut)}</td>
              <td className="text-right tabular-nums">{fmtCost(d.costUsd)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
