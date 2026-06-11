"use client";
import { useEffect, useState } from "react";

// Three row-density levels for the data tables, remembered across pages AND
// full reloads (localStorage + a module cache so navigation doesn't re-read or
// flicker). "comfortable" is the default.
export type Density = "comfortable" | "compact" | "condensed";

const KEY = "console.tableDensity";
const VALUES: Density[] = ["comfortable", "compact", "condensed"];
let cached: Density | null = null;

function readStored(): Density {
  if (typeof window === "undefined") return "comfortable";
  try {
    const v = window.localStorage.getItem(KEY) as Density | null;
    return v && VALUES.includes(v) ? v : "comfortable";
  } catch {
    return "comfortable";
  }
}

/** Shared + persisted table density. First render is the default (matches SSR)
 * to avoid a hydration mismatch; the stored value is applied after mount. */
export function useTableDensity(): [Density, (next: Density) => void] {
  const [density, setDensity] = useState<Density>(cached ?? "comfortable");

  useEffect(() => {
    if (cached === null) {
      cached = readStored();
      setDensity(cached);
    }
  }, []);

  const set = (next: Density) => {
    cached = next;
    setDensity(next);
    try {
      window.localStorage.setItem(KEY, next);
    } catch {
      /* storage disabled — keep the in-session value */
    }
  };

  return [density, set];
}

/** Tailwind cell padding for each density level. */
export const DENSITY_PADDING: Record<Density, string> = {
  comfortable: "py-2.5",
  compact: "py-1.5",
  condensed: "py-1",
};
