import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import UsersPage from "../app/(dash)/users/page";

vi.mock("next/navigation", () => ({ usePathname: () => "/users" }));

afterEach(() => vi.restoreAllMocks());

function stubFetch(body: unknown) {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
    ok: true, status: 200, json: async () => body,
    headers: new Headers({ "content-type": "application/json" }),
  } as unknown as Response));
}

describe("Users page (shell-refactored)", () => {
  it("renders the Users heading and the empty-state with 'No users.'", async () => {
    stubFetch({ result: [] });
    render(<UsersPage />);
    expect(await screen.findByRole("heading", { name: "Users" })).toBeInTheDocument();
    expect(await screen.findByText("No users.")).toBeInTheDocument();
  });

  it("no longer renders its own Sign out control (the shell owns it)", async () => {
    stubFetch({ result: [] });
    render(<UsersPage />);
    expect(screen.queryByRole("link", { name: /sign out/i })).not.toBeInTheDocument();
  });
});
