import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, waitFor } from "@testing-library/react";
// /apps is now a legacy redirect: OIDC login clients are managed inside each
// Application (/applications/<id>), so /apps forwards to the home app's detail.
import AppsRedirectPage from "../app/(dash)/apps/page";

const replace = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ replace }) }));
vi.mock("../lib/api", () => ({
  api: { get: vi.fn(async () => ({ id: "home-123", name: "llm-chat" })) },
  ApiError: class {},
}));

beforeEach(() => replace.mockClear());
afterEach(() => vi.restoreAllMocks());

describe("AppsRedirectPage", () => {
  it("redirects /apps to the home application's detail", async () => {
    render(<AppsRedirectPage />);
    await waitFor(() => expect(replace).toHaveBeenCalledWith("/applications/home-123"));
  });
});
