// URL scheme allowlists. Claude's markdown is semi-trusted, so a `.pdf` link or
// a ```pdf fence could carry a `javascript:`/`file:`/`data:` URL — never embed
// or open those.

/** http/https only — for embedding (iframe/img). null if unsafe. */
export function safeEmbedUrl(raw: string): string | null {
  try {
    const u = new URL(raw);
    return u.protocol === "http:" || u.protocol === "https:" ? u.href : null;
  } catch {
    return null;
  }
}

/** http/https/mailto — for opening in the system browser. null if unsafe. */
export function safeOpenUrl(raw: string): string | null {
  try {
    const u = new URL(raw);
    return ["http:", "https:", "mailto:"].includes(u.protocol) ? u.href : null;
  } catch {
    return null;
  }
}
