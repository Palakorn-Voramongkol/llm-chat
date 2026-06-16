"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Boxes } from "lucide-react";
import { cn } from "@/lib/utils";
import { NAV, isActive } from "./nav";
import { OperatorBadge } from "./OperatorBadge";

export function Sidebar() {
  const pathname = usePathname();
  return (
    <nav
      aria-label="Primary"
      className="bg-sidebar text-sidebar-foreground border-sidebar-border flex w-[60px] shrink-0 flex-col items-center gap-1.5 border-r py-3"
    >
      <div className="bg-sidebar-accent text-sidebar-foreground mb-3 flex size-[34px] items-center justify-center rounded-[10px]">
        <Boxes className="size-5" />
      </div>
      {NAV.map((item) => {
        const active = isActive(pathname, item.match);
        const Icon = item.icon;
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
      })}
      {/* Account menu pinned to the bottom of the ribbon (console-shell mockup). */}
      <div className="mt-auto flex flex-col items-center pt-1.5">
        <OperatorBadge />
      </div>
    </nav>
  );
}
