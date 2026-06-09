"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Boxes } from "lucide-react";
import { cn } from "@/lib/utils";
import { NAV, isActive } from "./nav";

export function Sidebar() {
  const pathname = usePathname();
  return (
    <nav
      aria-label="Primary"
      className="flex w-[60px] shrink-0 flex-col items-center gap-1.5 bg-linear-to-b from-[#5b53e8] to-[#8b3df0] py-3 shadow-[2px_0_16px_rgba(91,83,232,0.22)]"
    >
      <div className="mb-3 flex size-[34px] items-center justify-center rounded-[10px] bg-white/15 text-white">
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
              active ? "bg-white/20 text-white" : "text-white/70 hover:bg-white/10 hover:text-white",
            )}
          >
            <Icon className="size-[22px]" />
            {active && (
              <span className="absolute -left-3 top-2.5 bottom-2.5 w-[3px] rounded-r-[3px] bg-white" />
            )}
          </Link>
        );
      })}
    </nav>
  );
}
