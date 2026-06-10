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

// Route the api.get fan-out by path: stats resolves; the best-effort status /
// events reads reject so their cards degrade (the page must stay healthy).
function stub(stats: Stats) {
  vi.spyOn(api, "get").mockImplementation((path: string) => {
    if (path.startsWith("/api/stats")) return Promise.resolve(stats as never);
    return Promise.reject(new Error("unavailable in test"));
  });
}

describe("dashboard cards", () => {
  it("renders each count and deep-links into its section", async () => {
    stub({ humans: 18, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    // "18" and "Humans" also appear in the donut legend — assert presence, not uniqueness.
    expect((await screen.findAllByText("18")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("Humans").length).toBeGreaterThan(0);
    expect(screen.getByText("Machine accounts")).toBeInTheDocument();
    // each card deep-links to its area
    expect(screen.getByRole("link", { name: /Humans/ })).toHaveAttribute("href", "/users");
    expect(screen.getByRole("link", { name: /Apps/ })).toHaveAttribute("href", "/apps");
    expect(screen.getByRole("link", { name: /Roles/ })).toHaveAttribute("href", "/roles");
  });

  it("shows an em-dash for a failed (null) count", async () => {
    stub({ humans: null, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    expect((await screen.findAllByText("—")).length).toBeGreaterThan(0);
  });

  it("degrades the activity chart when audit events are unavailable", async () => {
    stub({ humans: 18, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    expect(await screen.findByText("Audit events unavailable")).toBeInTheDocument();
  });

  it("renders quick-action links including Sessions", async () => {
    stub({ humans: 18, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true });
    render(<DashboardPage />);
    expect(await screen.findByRole("link", { name: /Sessions/ }))
      .toHaveAttribute("href", "/sessions");
    expect(screen.getByRole("link", { name: /View audit log/ }))
      .toHaveAttribute("href", "/audit");
  });
});

describe("dash index landing", () => {
  it("redirects / to /dashboard", async () => {
    const redirect = vi.fn();
    vi.doMock("next/navigation", () => ({ redirect }));
    const { default: DashIndex } = await import("../app/(dash)/page");
    DashIndex();
    expect(redirect).toHaveBeenCalledWith("/dashboard");
  });
});
