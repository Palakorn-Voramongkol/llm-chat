import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import Page from "./page";

afterEach(() => vi.restoreAllMocks());

describe("custom login page", () => {
  it("with ?authRequest, posts credentials and follows the callback", async () => {
    const assign = vi.fn();
    vi.stubGlobal("location", {
      search: "?authRequest=AR_1",
      set href(v: string) { assign(v); },
    } as unknown as Location);
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ callbackUrl: "http://host/callback?code=x" }),
        { status: 200, headers: { "content-type": "application/json" } }));
    vi.stubGlobal("fetch", fetchMock);
    render(<Page />);
    fireEvent.change(await screen.findByPlaceholderText(/email or username/i), { target: { value: "a@b.c" } });
    fireEvent.change(screen.getByPlaceholderText(/password/i), { target: { value: "pw123456" } });
    fireEvent.click(screen.getByRole("button", { name: /sign in/i }));
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith("/api/login", expect.objectContaining({ method: "POST" })));
  });
});
