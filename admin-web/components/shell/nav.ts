import {
  LayoutDashboard, Users, ShieldCheck, AppWindow, Building2, ScrollText,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  /** lucide icon component for the activity-bar ribbon. */
  icon: LucideIcon;
  /** human label (sidebar tooltip + topbar breadcrumb). */
  label: string;
  /** route this item navigates to. */
  href: string;
  /** path prefix that marks this item active (child routes included). */
  match: string;
}

// Single source of nav truth (spec §2): adding an area = append one entry
// + one page.tsx. Order is the build sequence from spec §15.
export const NAV: NavItem[] = [
  { icon: LayoutDashboard, label: "Dashboard", href: "/dashboard", match: "/dashboard" },
  { icon: Users, label: "Users", href: "/users", match: "/users" },
  { icon: ShieldCheck, label: "Roles", href: "/roles", match: "/roles" },
  { icon: AppWindow, label: "Applications", href: "/apps", match: "/apps" },
  { icon: Building2, label: "Project & Org", href: "/settings", match: "/settings" },
  { icon: ScrollText, label: "Audit", href: "/audit", match: "/audit" },
];

/** True when `pathname` is `match` or a child route of it. */
export function isActive(pathname: string, match: string): boolean {
  return pathname === match || pathname.startsWith(`${match}/`);
}
