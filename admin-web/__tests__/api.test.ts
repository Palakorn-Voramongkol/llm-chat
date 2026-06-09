import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { api, ApiError } from "../lib/api";
import type { Stats } from "../lib/types";

function mockFetch(status: number, body: unknown, ok = status < 400) {
  return vi.fn().mockResolvedValue({
    ok,
    status,
    json: async () => body,
    text: async () => JSON.stringify(body),
    headers: new Headers({ "content-type": "application/json" }),
  } as unknown as Response);
}

beforeEach(() => {
  // jsdom has no location.assign by default; stub it
  Object.defineProperty(window, "location", {
    value: { assign: vi.fn(), href: "" },
    writable: true,
  });
});
afterEach(() => vi.restoreAllMocks());

describe("api client", () => {
  it("GET sends credentials:'include' and same-origin /api path, returns parsed JSON", async () => {
    const f = mockFetch(200, { result: [{ id: "u1", userName: "a" }] });
    vi.stubGlobal("fetch", f);
    const out = await api.get<{ result: unknown[] }>("/api/users?q=a");
    expect(f).toHaveBeenCalledTimes(1);
    const [url, init] = f.mock.calls[0];
    expect(url).toBe("/api/users?q=a");
    expect(init.credentials).toBe("include");
    expect(out.result).toHaveLength(1);
  });

  it("POST serializes JSON body + sets content-type", async () => {
    const f = mockFetch(200, { userId: "u9" });
    vi.stubGlobal("fetch", f);
    await api.post("/api/users/machine", { userName: "bot", name: "bot" });
    const [, init] = f.mock.calls[0];
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ userName: "bot", name: "bot" });
    expect((init.headers as Record<string, string>)["Content-Type"]).toBe("application/json");
  });

  it("maps admin-api {code,message} error JSON to ApiError", async () => {
    const f = mockFetch(409, { code: "AlreadyExists", message: "user exists" });
    vi.stubGlobal("fetch", f);
    await expect(api.post("/api/users/machine", {})).rejects.toMatchObject({
      name: "ApiError",
      status: 409,
      code: "AlreadyExists",
      message: "user exists",
    });
  });

  it("on 401 redirects to /login (full-page nav) and throws", async () => {
    const f = mockFetch(401, { code: "Unauthorized", message: "no session" });
    vi.stubGlobal("fetch", f);
    await expect(api.get("/api/me")).rejects.toBeInstanceOf(ApiError);
    expect(window.location.assign).toHaveBeenCalledWith("/login");
  });
});

describe("stats type", () => {
  it("parses a /api/stats body into Stats (null counts allowed)", () => {
    const body: Stats = {
      humans: 18, machines: 6, roles: 3, grants: 40, apps: 3, tokenHealthy: true,
    };
    expect(body.humans).toBe(18);
    const degraded: Stats = {
      humans: null, machines: null, roles: null, grants: null, apps: null,
      tokenHealthy: false,
    };
    expect(degraded.apps).toBeNull();
  });
});
