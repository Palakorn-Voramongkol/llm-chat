"use client";
import { useEffect, useState } from "react";

// The filter panel's open/closed state is REMEMBERED across page navigation AND
// full reloads. It is persisted in localStorage and cached in a module variable
// so navigating between table pages doesn't re-read storage or flicker.
const KEY = "console.filterOpen";
let cached: boolean | null = null;

function readStored(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

/** Like useState<boolean> but shared + persisted (survives reload). To avoid a
 * hydration mismatch the first render is always `false` (matching SSR); the
 * stored value is applied right after mount. Accepts a boolean or updater so
 * existing `setOpen(o => !o)` / `setOpen(false)` call sites keep working. */
export function useFilterOpen(): [boolean, (next: boolean | ((open: boolean) => boolean)) => void] {
  const [open, setOpen] = useState(cached ?? false);

  useEffect(() => {
    if (cached === null) {
      cached = readStored();
      setOpen(cached);
    }
  }, []);

  const set = (next: boolean | ((open: boolean) => boolean)) =>
    setOpen((prev) => {
      const value = typeof next === "function" ? next(prev) : next;
      cached = value;
      try {
        window.localStorage.setItem(KEY, value ? "1" : "0");
      } catch {
        /* storage disabled / full — keep the in-session value */
      }
      return value;
    });

  return [open, set];
}
