import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("kabytech login page", () => {
  it("shows Sign in when /api/me is 401", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 401 })));
    render(<Page />);
    expect(await screen.findByRole("link", { name: /sign in/i })).toHaveAttribute("href", "/login");
  });

  it("shows the user and Logout when /api/me returns a user", async () => {
    vi.stubGlobal("fetch", vi.fn(async () =>
      new Response(JSON.stringify({ userId: "u1", name: "Ada", roles: ["chat.user"] }),
        { status: 200, headers: { "content-type": "application/json" } })));
    render(<Page />);
    expect(await screen.findByText("Ada")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /log out/i })).toHaveAttribute("href", "/logout");
  });
});
