"use client";
import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { api } from "@/lib/api";
import type { Project } from "@/lib/types";

// Legacy route. OIDC login clients are now managed inside each Application
// (/applications/<id>). Redirect to the home application's detail so old
// bookmarks still land on the platform project's clients.
export default function AppsRedirectPage() {
  const router = useRouter();
  useEffect(() => {
    let alive = true;
    api
      .get<Project>("/api/project")
      .then((p) => { if (alive) router.replace(p?.id ? `/applications/${p.id}` : "/applications"); })
      .catch(() => { if (alive) router.replace("/applications"); });
    return () => { alive = false; };
  }, [router]);
  return null;
}
