import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import DashLayout from "../app/(dash)/layout";

vi.mock("next/navigation", () => ({
  usePathname: () => "/users",
  useRouter: () => ({ push: vi.fn() }),
}));
vi.mock("../lib/api", () => ({
  api: { get: () => Promise.resolve({ userId: "u1", name: "x", roles: [] }) },
  ApiError: class {},
}));

describe("(dash) shell layout", () => {
  it("renders the sidebar, topbar, and the page slot children", () => {
    render(<DashLayout><div data-testid="page-slot">PAGE</div></DashLayout>);
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByText("Console /")).toBeInTheDocument();
    expect(screen.getByTestId("page-slot")).toHaveTextContent("PAGE");
  });
});
