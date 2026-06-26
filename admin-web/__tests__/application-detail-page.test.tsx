import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ApplicationDetailPage from "../app/(dash)/applications/[id]/page";

vi.mock("next/navigation", () => ({ useParams: () => ({ id: "p1" }) }));

// The clients endpoint returns the original redirect URI on the first fetch and
// an "edited" one on every later fetch — so a reload (after an edit) yields a
// different client object than the one the row was selected with. This is what
// distinguishes the live-derivation fix from the stale-selection bug.
let appsCall = 0;
function appsResult() {
  appsCall++;
  const uri = appsCall >= 2 ? "https://portal.example/EDITED" : "https://portal.example/callback";
  return {
    result: [
      {
        id: "a1",
        name: "web-portal",
        oidcConfig: {
          clientId: "client-123",
          appType: "OIDC_APP_TYPE_WEB",
          authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC",
          grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
          responseTypes: ["OIDC_RESPONSE_TYPE_CODE"],
          redirectUris: [uri],
        },
      },
    ],
  };
}

const get = vi.fn(async (url: string) => {
  if (url === "/api/projects") return { result: [{ id: "p1", name: "llm-chat" }] };
  if (url === "/api/projects/p1/apps") return appsResult();
  if (url === "/api/projects/p1/roles") return { result: [] };
  if (url === "/api/projects/p1/grants") return { result: [] };
  return {};
});

vi.mock("@/lib/api", () => ({
  api: {
    get: (url: string) => get(url),
    put: vi.fn(async () => ({})),
    del: vi.fn(async () => ({})),
    post: vi.fn(async () => ({})),
  },
  ApiError: class ApiError extends Error {},
}));

beforeEach(() => {
  appsCall = 0;
  get.mockClear();
});
afterEach(() => vi.restoreAllMocks());

describe("ApplicationDetailPage", () => {
  it("lists the application's login clients and opens the detail panel on select", async () => {
    render(<ApplicationDetailPage />);
    await waitFor(() => expect(screen.getByText("web-portal")).toBeInTheDocument());
    // the redirect URI is only rendered inside the detail panel (panel closed initially)
    expect(screen.queryByText("https://portal.example/callback")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /web-portal/i }));
    await waitFor(() =>
      expect(screen.getByText("https://portal.example/callback")).toBeInTheDocument(),
    );
  });

  it("reflects an edit in the open detail panel (live derivation, not the stale selection)", async () => {
    render(<ApplicationDetailPage />);
    await waitFor(() => expect(screen.getByText("web-portal")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /web-portal/i }));
    await waitFor(() =>
      expect(screen.getByText("https://portal.example/callback")).toBeInTheDocument(),
    );
    // Open Edit and save. No field change is needed: the reloaded clients list
    // returns the updated client, and the panel must reflect it.
    fireEvent.click(screen.getByRole("button", { name: /^Edit$/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Save" }));
    // If the panel rendered the stale selection object instead of the live list
    // entry, it would still show the original URI and this would fail.
    await waitFor(() =>
      expect(screen.getByText("https://portal.example/EDITED")).toBeInTheDocument(),
    );
    expect(screen.queryByText("https://portal.example/callback")).not.toBeInTheDocument();
  });
});
