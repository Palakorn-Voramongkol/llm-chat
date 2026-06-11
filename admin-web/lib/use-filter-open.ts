"use client";
import { useState } from "react";

// Module-level so the filter panel's open/closed state is REMEMBERED as the
// operator moves between pages (Users -> Apps -> Roles -> Audit). It lives
// outside React state on purpose: a plain module variable survives client-side
// navigation (the module stays loaded) without a localStorage hydration dance.
// Resets to closed on a full page reload.
let remembered = false;

/** Like useState<boolean> but the value is shared + remembered across every
 * table page for the lifetime of the SPA session. Accepts a boolean or an
 * updater, so existing `setOpen(o => !o)` / `setOpen(false)` call sites work. */
export function useFilterOpen(): [boolean, (next: boolean | ((open: boolean) => boolean)) => void] {
  const [open, setOpen] = useState(remembered);
  const set = (next: boolean | ((open: boolean) => boolean)) =>
    setOpen((prev) => {
      const value = typeof next === "function" ? next(prev) : next;
      remembered = value;
      return value;
    });
  return [open, set];
}
