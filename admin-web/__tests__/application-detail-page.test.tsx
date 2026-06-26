import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ApplicationDetailPage from "../app/(dash)/applications/[id]/page";

vi.mock("next/navigation", () => ({ useParams: () => ({ id: "p1" }) }));

const get = vi.fn(async (url: string) => {
  if (url === "/api/projects") return { result: [{ id: "p1", name: "llm-chat" }] };
  if (url === "/api/projects/p1/apps")
    return {
      result: [
        {
          id: "a1",
          name: "web-portal",
          oidcConfig: {
            clientId: "client-123",
            appType: "OIDC_APP_TYPE_WEB",
            redirectUris: ["https://portal.example/callback"],
          },
        },
      ],
    };
  if (url === "/api/projects/p1/roles") return { result: [] };
  if (url === "/api/projects/p1/grants") return { result: [] };
  return {};
});

vi.mock("@/lib/api", () => ({
  api: { get: (url: string) => get(url), del: vi.fn(async () => ({})), post: vi.fn(async () => ({})) },
  ApiError: class ApiError extends Error {},
}));

beforeEach(() => get.mockClear());
afterEach(() => vi.restoreAllMocks());

describe("ApplicationDetailPage", () => {
  it("lists the application's login clients and opens the detail panel on select", async () => {
    render(<ApplicationDetailPage />);
    // the application's login client appears in the list
    await waitFor(() => expect(screen.getByText("web-portal")).toBeInTheDocument());
    // the redirect URI is only rendered inside the detail panel (panel closed initially)
    expect(screen.queryByText("https://portal.example/callback")).not.toBeInTheDocument();
    // selecting the client opens the detail panel
    fireEvent.click(screen.getByRole("button", { name: /web-portal/i }));
    await waitFor(() =>
      expect(screen.getByText("https://portal.example/callback")).toBeInTheDocument(),
    );
  });
});
