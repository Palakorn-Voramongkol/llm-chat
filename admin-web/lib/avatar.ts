/** First letter of up to two words, uppercased; "?" when empty. */
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  return parts.slice(0, 2).map((p) => p[0]!.toUpperCase()).join("");
}

// Full class strings (Tailwind must see literal classes — no string building).
const GRADIENTS = [
  "from-indigo-500 to-violet-500",
  "from-sky-500 to-emerald-500",
  "from-amber-500 to-rose-500",
  "from-emerald-500 to-teal-500",
  "from-fuchsia-500 to-pink-500",
  "from-blue-500 to-cyan-500",
] as const;

/** Deterministic per-seed gradient pair (simple char-code hash). */
export function avatarGradient(seed: string): string {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) {
    hash = (hash * 31 + seed.charCodeAt(i)) | 0;
  }
  return GRADIENTS[Math.abs(hash) % GRADIENTS.length];
}
