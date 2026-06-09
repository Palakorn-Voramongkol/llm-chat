"use client";
import Link from "next/link";
import { usePathname } from "next/navigation";
import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { isActive } from "./nav";

interface NavLinkProps {
  href: string;
  match: string;
  label: string;
  icon?: LucideIcon;
}

export function NavLink({ href, match, label, icon: Icon }: NavLinkProps) {
  const pathname = usePathname();
  const active = isActive(pathname, match);
  return (
    <Link
      href={href}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm transition-colors",
        active
          ? "bg-indigo-500/10 font-semibold text-indigo-600"
          : "text-foreground hover:bg-muted",
      )}
    >
      {Icon && (
        <Icon className={cn("size-4", active ? "text-indigo-600" : "text-muted-foreground")} />
      )}
      {label}
    </Link>
  );
}
