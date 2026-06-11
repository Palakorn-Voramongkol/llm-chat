import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
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
  const statusBody = (name: string, roles: string[] = []) => ({
    operator: { userId: "u1", name, roles },
    session: { expiresAt: null },
    health: { zitadel: true },
    capabilities: { events: true, chatSessions: true },
  });

  it("renders the operator name once /api/status resolves", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => statusBody("palakorn", ["chat.admin"]),
      headers: new Headers({ "content-type": "application/json" }),
    } as unknown as Response));
    render(<OperatorBadge />);
    expect(await screen.findByText("palakorn")).toBeInTheDocument();
  });

  it("shows a sign-out link to /logout in the account menu", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true, status: 200,
      json: async () => statusBody("x"),
      headers: new Headers({ "content-type": "application/json" }),
    } as unknown as Response));
    render(<OperatorBadge />);
    // Radix renders menu items into a portal only once the trigger is opened.
    const trigger = screen.getByRole("button", { name: /account menu/i });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.click(trigger);
    const link = await screen.findByTestId("signout");
    expect(link).toHaveAttribute("href", "/logout");
  });
});
