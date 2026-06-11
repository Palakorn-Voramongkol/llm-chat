"use client";
import { useRef, useState, type ReactNode } from "react";
import { Check, Copy, X } from "lucide-react";
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

/** A small copy-to-clipboard button that copies `getText()` when clicked, and
 * briefly flips to a check. Hidden until the row is hovered/focused. */
function CopyButton({ getText }: { getText: () => string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-xs"
      aria-label={copied ? "Copied" : "Copy value"}
      className="shrink-0 opacity-0 transition-opacity group-hover/field:opacity-100 focus-visible:opacity-100"
      onClick={async () => {
        const text = getText().trim();
        if (!text || text === "—") return;
        try {
          await navigator.clipboard.writeText(text);
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1200);
        } catch {
          /* clipboard blocked — no-op */
        }
      }}
    >
      {copied ? <Check className="size-3 text-success" /> : <Copy className="size-3" />}
    </Button>
  );
}

/** A key/value row for a panel (light card surface). Long values wrap/break;
 * `mono` for ids. A copy button appears on hover and copies the value text. */
export function PanelField({
  label,
  mono,
  children,
}: {
  label: string;
  mono?: boolean;
  children: ReactNode;
}) {
  const valueRef = useRef<HTMLSpanElement>(null);
  return (
    <div className="group/field grid grid-cols-[6.5rem_1fr_auto] items-start gap-x-1.5 gap-y-1 py-1 text-sm">
      <span className="text-muted-foreground py-0.5">{label}</span>
      <span ref={valueRef} className={mono ? "font-mono text-xs break-all py-0.5" : "break-words py-0.5"}>
        {children}
      </span>
      <CopyButton getText={() => valueRef.current?.textContent ?? ""} />
    </div>
  );
}
