function BrandMark() {
  return (
    <svg width="28" height="28" viewBox="0 0 28 28" fill="none" aria-hidden="true">
      <rect x="6" y="6" width="16" height="16" rx="5" fill="url(#kbg)"
        transform="rotate(45 14 14)" />
      <defs>
        <linearGradient id="kbg" x1="0" y1="0" x2="28" y2="28" gradientUnits="userSpaceOnUse">
          <stop stopColor="#93c5fd" />
          <stop offset="1" stopColor="#2563eb" />
        </linearGradient>
      </defs>
    </svg>
  );
}

/** Split-screen auth shell: a deep-navy brand panel (desktop) / band (mobile)
 * beside a clean form column. Same API as before ({title, subtitle, children}),
 * so /login, /invite, /accept render unchanged. */
export function AuthCard({ title, subtitle, children }: {
  title: string; subtitle?: string; children: React.ReactNode;
}) {
  return (
    <div className="flex min-h-screen">
      {/* Brand panel — desktop only */}
      <aside className="relative hidden w-1/2 overflow-hidden bg-gradient-to-br from-slate-900 via-slate-800 to-slate-950 md:flex md:flex-col md:justify-between lg:w-[45%]">
        <div aria-hidden className="pointer-events-none absolute -right-24 -top-24 h-72 w-72 rounded-full bg-blue-500/10 blur-3xl" />
        <div aria-hidden className="pointer-events-none absolute -bottom-28 -left-20 h-80 w-80 rounded-full bg-blue-600/10 blur-3xl" />
        <div className="relative flex items-center gap-2.5 p-10">
          <BrandMark />
          <span className="text-lg font-semibold tracking-tight text-white">kabytech</span>
        </div>
        <div className="relative px-10">
          <h2 className="max-w-sm text-4xl font-semibold leading-tight text-white">
            Your AI workspace, one login.
          </h2>
          <p className="mt-4 max-w-sm text-base leading-relaxed text-slate-300">
            Secure access to your team&apos;s AI chat — invite, join, and sign in,
            all in one place.
          </p>
        </div>
        <div className="relative p-10 text-sm text-slate-400">© kabytech</div>
      </aside>

      {/* Form column */}
      <main className="flex flex-1 flex-col bg-white">
        {/* Brand band — mobile only */}
        <div className="flex items-center gap-2.5 bg-slate-900 px-5 py-4 md:hidden">
          <BrandMark />
          <span className="font-semibold tracking-tight text-white">kabytech</span>
        </div>
        <div className="flex flex-1 items-center justify-center p-6 sm:p-10">
          <div className="w-full max-w-sm">
            <h1 className="text-2xl font-semibold tracking-tight text-slate-900">{title}</h1>
            {subtitle && <p className="mt-1.5 text-sm text-slate-500">{subtitle}</p>}
            <div className="mt-6">{children}</div>
          </div>
        </div>
      </main>
    </div>
  );
}

export const inputCls =
  "w-full rounded-lg border border-slate-300 px-3.5 py-2.5 text-sm text-slate-900 outline-none transition placeholder:text-slate-400 focus:border-blue-500 focus:ring-2 focus:ring-blue-500/30";
export const btnCls =
  "w-full rounded-lg bg-blue-600 px-4 py-2.5 text-sm font-semibold text-white shadow-sm transition hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500/40 disabled:opacity-50";
