"use client";
import { usePathname } from "next/navigation";
import { NAV, isActive } from "./nav";
import { ThemeToggle } from "./ThemeToggle";
import { NotificationBell } from "./NotificationBell";

export function Topbar() {
  const pathname = usePathname();
  const current = NAV.find((n) => isActive(pathname, n.match));
  return (
    <header className="bg-card flex h-15 shrink-0 items-center gap-3 border-b px-6 py-3">
      <span className="text-muted-foreground text-sm tracking-tight">
        Console / <span className="text-foreground font-medium">{current?.label ?? "Home"}</span>
      </span>
      <span className="flex-1" />
      <NotificationBell />
      <ThemeToggle />
    </header>
  );
}
