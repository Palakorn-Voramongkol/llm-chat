"use client";
import type { ReactNode } from "react";
import { X } from "lucide-react";
import { Button } from "@/components/ui/button";

/**
 * A slide-in detail panel that sits to the RIGHT of a table. The table stays
 * visible for row-to-row scanning; selecting a row fills this panel with the
 * record's full detail — replacing "open the ⋯ menu → open a modal → read".
 * Render it as the trailing child of a horizontal flex row next to the table,
 * and only when something is selected (open).
 */
export function DetailPanel({
  open,
  title,
  subtitle,
  onClose,
  children,
}: {
  open: boolean;
  title: ReactNode;
  subtitle?: ReactNode;
  onClose: () => void;
  children: ReactNode;
}) {
  if (!open) return null;
  return (
    <aside
      aria-label="Detail panel"
      className="bg-card animate-in slide-in-from-right-4 fade-in-0 flex w-[22rem] shrink-0 flex-col overflow-hidden rounded-xl border shadow-sm duration-200"
    >
      <div className="flex items-start justify-between gap-2 border-b px-4 py-3">
        <div className="min-w-0">
          <div className="truncate font-semibold">{title}</div>
          {subtitle && (
            <div className="text-muted-foreground truncate text-xs">{subtitle}</div>
          )}
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onClose}
          aria-label="Close detail panel"
          className="shrink-0"
        >
          <X className="size-4" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-4 py-3">{children}</div>
    </aside>
  );
}

/** A titled section within a panel. */
export function PanelSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="mb-5 last:mb-0">
      <h3 className="text-muted-foreground mb-2 text-[11px] font-semibold tracking-wide uppercase">
        {title}
      </h3>
      {children}
    </section>
  );
}

/** A key/value row for a panel (light card surface). Long values wrap/break;
 * `mono` for ids. */
export function PanelField({
  label,
  mono,
  children,
}: {
  label: string;
  mono?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-1 py-1 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className={mono ? "font-mono text-xs break-all" : "break-words"}>
        {children}
      </span>
    </div>
  );
}
