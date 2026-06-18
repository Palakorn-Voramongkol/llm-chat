"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Boxes, Settings, type LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { NAV, isActive, type NavItem } from "./nav";
import { OperatorBadge } from "./OperatorBadge";

export function Sidebar() {
  const pathname = usePathname();
  // The "/settings" (Project & Org) item lives at the BOTTOM of the ribbon,
  // above the account avatar, rendered with a gear icon (console-shell mockup).
  // It stays in NAV (single source of truth for the Topbar breadcrumb) — only
  // its PLACEMENT and ribbon icon differ here.
  const topItems = NAV.filter((item) => item.href !== "/settings");
  const settingsItem = NAV.find((item) => item.href === "/settings");

  const renderItem = (item: NavItem, icon: LucideIcon) => {
    const active = isActive(pathname, item.match);
    const Icon = icon;
    return (
      <Link
        key={item.href}
        href={item.href}
        title={item.label}
        aria-label={item.label}
        aria-current={active ? "page" : undefined}
        className={cn(
          "relative flex size-11 items-center justify-center rounded-xl transition-colors",
          active
            ? "bg-sidebar-accent text-sidebar-foreground"
            : "text-sidebar-foreground/70 hover:bg-sidebar-accent/60 hover:text-sidebar-foreground",
        )}
      >
        <Icon className="size-[22px]" />
        {active && (
          <span className="bg-sidebar-foreground absolute top-2.5 bottom-2.5 -left-3 w-[3px] rounded-r-[3px]" />
        )}
      </Link>
    );
  };

  return (
    <nav
      aria-label="Primary"
      className="bg-sidebar text-sidebar-foreground border-sidebar-border flex w-[60px] shrink-0 flex-col items-center gap-1.5 border-r py-3"
    >
      <div className="bg-sidebar-accent text-sidebar-foreground mb-3 flex size-[34px] items-center justify-center rounded-[10px]">
        <Boxes className="size-5" />
      </div>
      {topItems.map((item) => renderItem(item, item.icon))}
      {/* Settings (Project & Org) gear + account menu pinned to the bottom of
          the ribbon (console-shell mockup). */}
      <div className="mt-auto flex flex-col items-center gap-1.5 pt-1.5">
        {settingsItem && renderItem(settingsItem, Settings)}
        <OperatorBadge />
      </div>
    </nav>
  );
}
