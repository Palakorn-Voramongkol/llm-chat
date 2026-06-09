import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
// Canonical route is /apps (the single nav source of truth in
// components/shell/nav.ts + the dashboard deep-link), so the page lives at
// app/(dash)/apps/page.tsx.
import ApplicationsPage from "../app/(dash)/apps/page";

vi.mock("next/navigation", () => ({ usePathname: () => "/apps" }));

function mockJson(body: unknown) {
  return { ok: true, status: 200, json: async () => body, text: async () => JSON.stringify(body), headers: new Headers() } as unknown as Response;
}

beforeEach(() => {
  Object.defineProperty(window, "location", { value: { assign: vi.fn(), href: "" }, writable: true });
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (url.startsWith("/api/me")) return mockJson({ userId: "o1", name: "Op", roles: ["chat.admin"] });
    if (url.startsWith("/api/apps")) return mockJson({ result: [
      { id: "a1", name: "Chat", oidcConfig: { clientId: "c1", appType: "OIDC_APP_TYPE_WEB" } },
    ] });
    return mockJson({});
  }));
});
afterEach(() => vi.restoreAllMocks());

describe("ApplicationsPage", () => {
  it("loads and lists apps + shows create button", async () => {
    render(<ApplicationsPage />);
    await waitFor(() => expect(screen.getByText("Chat")).toBeInTheDocument());
    expect(screen.getByTestId("create-app")).toBeInTheDocument();
  });
});
