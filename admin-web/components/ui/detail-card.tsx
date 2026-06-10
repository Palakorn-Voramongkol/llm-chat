"use client";
import type { ReactNode } from "react";

/**
 * Two-column key/value layout for the full-record tooltips on the Users and
 * Apps tables. Rendered inside a (dark) TooltipContent, so colors inherit from
 * the tooltip surface; labels are dimmed, values wrap/break so long ids and
 * redirect URIs never overflow the card.
 */
export function DetailCard({ children }: { children: ReactNode }) {
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs leading-relaxed">
      {children}
    </dl>
  );
}

export function DetailRow({
  label,
  mono,
  children,
}: {
  label: string;
  mono?: boolean;
  children: ReactNode;
}) {
  return (
    <>
      <dt className="opacity-60 whitespace-nowrap">{label}</dt>
      <dd className={mono ? "font-mono break-all" : "break-words"}>{children}</dd>
    </>
  );
}
