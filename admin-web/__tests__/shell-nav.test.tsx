import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { NAV, isActive } from "../components/shell/nav";
import { NavLink } from "../components/shell/NavLink";

vi.mock("next/navigation", () => ({ usePathname: () => "/users" }));

describe("NAV source of truth", () => {
  it("lists the six v1 areas with hrefs in order", () => {
    expect(NAV.map((n) => n.href)).toEqual([
      "/dashboard", "/users", "/roles", "/apps", "/settings", "/audit",
    ]);
    expect(NAV.map((n) => n.label)).toEqual([
      "Dashboard", "Users", "Roles", "Applications", "Project & Org", "Audit",
    ]);
  });

  it("isActive matches the area and its child routes by prefix", () => {
    expect(isActive("/users", "/users")).toBe(true);
    expect(isActive("/users/abc-123", "/users")).toBe(true);
    expect(isActive("/roles", "/users")).toBe(false);
  });
});

describe("NavLink", () => {
  it("marks the current route with aria-current=page", () => {
    render(<NavLink href="/users" match="/users" label="Users" />);
    expect(screen.getByRole("link", { name: "Users" }))
      .toHaveAttribute("aria-current", "page");
  });

  it("does not mark a non-current route", () => {
    render(<NavLink href="/roles" match="/roles" label="Roles" />);
    expect(screen.getByRole("link", { name: "Roles" }))
      .not.toHaveAttribute("aria-current");
  });
});
