import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { Sidebar } from "../components/shell/Sidebar";
import { Topbar } from "../components/shell/Topbar";

vi.mock("next/navigation", () => ({
  usePathname: () => "/users",
  useRouter: () => ({ push: vi.fn() }),
}));
vi.mock("../lib/api", () => ({
  api: { get: () => Promise.resolve({ userId: "u1", name: "x", roles: [] }) },
  ApiError: class {},
}));

describe("Sidebar", () => {
  it("renders a labelled link for every NAV area", () => {
    render(<Sidebar />);
    for (const label of ["Dashboard", "Users", "Roles", "Applications", "OIDC Clients", "Project & Org", "Audit"]) {
      expect(screen.getByRole("link", { name: new RegExp(label, "i") })).toBeInTheDocument();
    }
  });

  it("marks the current area active", () => {
    render(<Sidebar />);
    expect(screen.getByRole("link", { name: /Users/i })).toHaveAttribute("aria-current", "page");
  });
});

describe("Topbar", () => {
  it("shows the current area in the breadcrumb", () => {
    render(<Topbar />);
    expect(screen.getByText("Users")).toBeInTheDocument();
  });
});
