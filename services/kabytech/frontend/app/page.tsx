"use client";
import { useEffect, useState } from "react";

type Me = { userId: string; name: string; roles: string[] };

export default function Page() {
  const [me, setMe] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch("/api/me")
      .then((r) => (r.ok ? r.json() : null))
      .then((d) => setMe(d))
      .catch(() => setMe(null))
      .finally(() => setLoading(false));
  }, []);

  return (
    <main className="flex min-h-screen items-center justify-center p-6">
      <div className="w-full max-w-sm rounded-xl border border-slate-200 bg-white p-8 shadow-sm">
        <h1 className="mb-6 text-xl font-semibold">kabytech</h1>
        {loading ? (
          <p className="text-slate-500">Loading…</p>
        ) : me ? (
          <div className="space-y-4">
            <p className="text-sm text-slate-500">Signed in as</p>
            <p className="text-lg font-medium">{me.name}</p>
            <a href="/logout"
              className="inline-block rounded-md bg-slate-900 px-4 py-2 text-sm font-medium text-white">
              Log out
            </a>
          </div>
        ) : (
          <a href="/login"
            className="inline-block rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white">
            Sign in
          </a>
        )}
      </div>
    </main>
  );
}
