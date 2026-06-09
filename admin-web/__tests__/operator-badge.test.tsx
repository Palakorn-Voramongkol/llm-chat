import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { OperatorBadge, initials } from "../components/shell/OperatorBadge";

afterEach(() => vi.restoreAllMocks());

describe("initials", () => {
  it("takes the first letter of up to two words, uppercased", () => {
    expect(initials("palakorn voramongkol")).toBe("PV");
    expect(initials("demo")).toBe("D");
    expect(initials("")).toBe("?");
  });
});

describe("OperatorBadge", () => {
  it("renders the operator name once /api/me resolves", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({ userId: "u1", name: "palakorn", roles: ["chat.admin"] }),
      headers: new Headers({ "content-type": "application/json" }),
    } as unknown as Response));
    render(<OperatorBadge />);
    expect(await screen.findByText("palakorn")).toBeInTheDocument();
  });

  it("renders a sign-out link to /logout", () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => ({ userId: "u1", name: "x", roles: [] }),
      headers: new Headers({ "content-type": "application/json" }),
    } as unknown as Response));
    render(<OperatorBadge />);
    expect(screen.getByRole("link", { name: /sign out/i }))
      .toHaveAttribute("href", "/logout");
  });
});
