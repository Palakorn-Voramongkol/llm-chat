import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("invite page", () => {
  it("posts the email and shows a success state", async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ ok: true, email: "a@b.c" }),
        { status: 200, headers: { "content-type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    render(<Page />);
    fireEvent.change(screen.getByPlaceholderText(/email/i), { target: { value: "a@b.c" } });
    fireEvent.click(screen.getByRole("button", { name: /send invite/i }));
    await waitFor(() => expect(screen.getByText(/invite sent/i)).toBeInTheDocument());
    expect(fetchMock).toHaveBeenCalledWith("/api/invite", expect.objectContaining({ method: "POST" }));
  });
});
