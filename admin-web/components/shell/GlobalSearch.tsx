"use client";
import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Boxes, Search, ShieldCheck, UserRound } from "lucide-react";
import {
  CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem,
  CommandList,
} from "@/components/ui/command";
import { avatarGradient, initials } from "@/lib/avatar";
import { api } from "@/lib/api";
import type { AppProject, AppProjectList, Role, RoleList, User, UserList } from "@/lib/types";
import { NAV } from "./nav";

interface RoleHit { key: string; displayName?: string; app: string; projectId: string }

/** Global "Search everything" — a ⌘K command palette in the top bar that
 * searches live Pages, Applications, Roles and Users, and navigates to them.
 * Data is fetched once on first open (best-effort; each source degrades alone).*/
export function GlobalSearch() {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [apps, setApps] = useState<AppProject[]>([]);
  const [roles, setRoles] = useState<RoleHit[]>([]);
  const [users, setUsers] = useState<User[]>([]);

  // ⌘K / Ctrl+K toggles the palette anywhere in the Console.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  // Fetch the searchable index on first open (cached thereafter).
  useEffect(() => {
    if (!open || loaded) return;
    setLoaded(true);
    api.get<UserList>("/api/users").then((r) => setUsers(r.result ?? [])).catch(() => {});
    api
      .get<AppProjectList>("/api/projects")
      .then(async (r) => {
        const list = r.result ?? [];
        setApps(list);
        // Each app's roles (parallel, best-effort) → a flat, app-tagged index.
        const perApp = await Promise.all(
          list.map(async (a): Promise<RoleHit[]> => {
            try {
              const rl = await api.get<RoleList>(`/api/projects/${a.id}/roles`);
              return (rl.result ?? []).map((role: Role) => ({
                key: role.key,
                displayName: role.displayName,
                app: a.name ?? a.id,
                projectId: a.id,
              }));
            } catch {
              return [];
            }
          }),
        );
        setRoles(perApp.flat());
      })
      .catch(() => {});
  }, [open, loaded]);

  const go = useCallback(
    (href: string) => {
      setOpen(false);
      router.push(href);
    },
    [router],
  );

  return (
    <>
      {/* Top-bar trigger styled as a search field (console-shell mockup). */}
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-label="Search everything"
        className="text-muted-foreground hover:bg-muted/60 hover:text-foreground flex h-9 w-64 items-center gap-2 rounded-lg border px-3 text-sm transition-colors"
      >
        <Search className="size-4 shrink-0" />
        <span className="flex-1 text-left">Search everything…</span>
        <kbd className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 font-mono text-[10px] font-medium">
          ⌘K
        </kbd>
      </button>

      <CommandDialog open={open} onOpenChange={setOpen} title="Search everything"
        description="Search pages, applications, roles and users.">
        <CommandInput placeholder="Search pages, applications, roles, users…" />
        <CommandList>
          <CommandEmpty>No results.</CommandEmpty>

          <CommandGroup heading="Pages">
            {NAV.map((n) => {
              const Icon = n.icon;
              return (
                <CommandItem key={n.href} value={`page ${n.label}`} onSelect={() => go(n.href)}>
                  <Icon className="text-muted-foreground" />
                  {n.label}
                </CommandItem>
              );
            })}
          </CommandGroup>

          {apps.length > 0 && (
            <CommandGroup heading="Applications">
              {apps.map((a) => (
                <CommandItem key={a.id} value={`application ${a.name ?? a.id}`}
                  onSelect={() => go(`/applications/${a.id}`)}>
                  <Boxes className="text-violet-600" />
                  {a.name ?? a.id}
                </CommandItem>
              ))}
            </CommandGroup>
          )}

          {roles.length > 0 && (
            <CommandGroup heading="Roles">
              {roles.map((r) => (
                <CommandItem key={`${r.projectId}:${r.key}`}
                  value={`role ${r.key} ${r.displayName ?? ""} ${r.app}`}
                  onSelect={() => go(`/applications/${r.projectId}`)}>
                  <ShieldCheck className="text-indigo-600" />
                  <span className="font-mono text-xs">{r.key}</span>
                  <span className="text-muted-foreground ml-auto text-xs">{r.app}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          )}

          {users.length > 0 && (
            <CommandGroup heading="Users">
              {users.map((u) => {
                const name = u.displayName || u.userName;
                return (
                  <CommandItem key={u.id}
                    value={`user ${u.userName} ${u.email ?? ""} ${u.displayName ?? ""}`}
                    onSelect={() => go("/users")}>
                    <span aria-hidden
                      className={`flex size-5 items-center justify-center rounded-full bg-linear-to-br text-[9px] font-bold text-white ${avatarGradient(u.id || u.userName)}`}>
                      {initials(name)}
                    </span>
                    {u.userName}
                    {u.email && <span className="text-muted-foreground ml-auto text-xs">{u.email}</span>}
                  </CommandItem>
                );
              })}
            </CommandGroup>
          )}
        </CommandList>
      </CommandDialog>
    </>
  );
}
