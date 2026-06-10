import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import DashboardPage from "../app/(dash)/dashboard/page";
import { api } from "../lib/api";
import type { Stats } from "../lib/types";

vi.mock("next/link", () => ({
  default: ({ href, children }: { href: string; children: React.ReactNode }) => (
    <a href={href}>{children}</a>
  ),
}));

beforeEach(() => {
  Object.defineProperty(window, "location", {
    value: { assign: vi.fn(), href: "" }, writable: true,
  });
});
afterEach(() => vi.restoreAllMocks());

function stub(stats: Stats) {
  vi.spyOn(api, "get").mockResolvedValue(stats as never);
}

describe("dashboard cards", () => {
  it("renders each count and deep-links into its section", async () => {
    stub({ humans: 18, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    expect(await screen.findByText("18")).toBeInTheDocument();
    expect(screen.getByText("Humans")).toBeInTheDocument();
    expect(screen.getByText("Machine accounts")).toBeInTheDocument();
    // each card deep-links to its area
    expect(screen.getByRole("link", { name: /Humans/ })).toHaveAttribute("href", "/users");
    expect(screen.getByRole("link", { name: /Apps/ })).toHaveAttribute("href", "/apps");
    expect(screen.getByRole("link", { name: /Roles/ })).toHaveAttribute("href", "/roles");
  });

  it("shows an em-dash for a failed (null) count", async () => {
    stub({ humans: null, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    expect(await screen.findByText("—")).toBeInTheDocument();
  });
});
