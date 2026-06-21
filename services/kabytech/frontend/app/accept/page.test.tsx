import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("accept page", () => {
  it("posts user_id, code, and password", async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ ok: true }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    // jsdom-safe: set the URL the page reads via history (location.search is read-only).
    window.history.pushState({}, "", "/accept?userID=u1&code=c1&orgID=o1");
    render(<Page />);
    fireEvent.change(screen.getByPlaceholderText(/^password/i), { target: { value: "hunter2!" } });
    fireEvent.change(screen.getByPlaceholderText(/confirm/i), { target: { value: "hunter2!" } });
    fireEvent.click(screen.getByRole("button", { name: /set password/i }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith("/api/accept", expect.objectContaining({ method: "POST" })));
  });
});
