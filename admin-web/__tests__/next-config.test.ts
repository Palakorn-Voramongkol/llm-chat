import { describe, it, expect, beforeEach } from "vitest";

describe("next.config same-origin proxy", () => {
  beforeEach(() => {
    process.env.ADMIN_API_ORIGIN = "http://localhost:7676";
  });

  it("rewrites /api/* and the OIDC nav routes to admin-api, no CORS", async () => {
    const mod = await import("../next.config");
    const cfg = mod.default;
    const rules = await cfg.rewrites!();
    // next.config.rewrites may return an array or {beforeFiles,...}
    const list = Array.isArray(rules) ? rules : rules.beforeFiles ?? [];
    const bySource = Object.fromEntries(list.map((r: any) => [r.source, r.destination]));

    expect(bySource["/api/:path*"]).toBe("http://localhost:7676/api/:path*");
    expect(bySource["/login"]).toBe("http://localhost:7676/login");
    expect(bySource["/callback"]).toBe("http://localhost:7676/callback");
    expect(bySource["/logout"]).toBe("http://localhost:7676/logout");
  });

  it("defaults ADMIN_API_ORIGIN to localhost:7676 when unset", async () => {
    delete process.env.ADMIN_API_ORIGIN;
    const mod = await import("../next.config?fresh=" + Date.now());
    const cfg = mod.default;
    const rules = await cfg.rewrites!();
    const list = Array.isArray(rules) ? rules : rules.beforeFiles ?? [];
    const bySource = Object.fromEntries(list.map((r: any) => [r.source, r.destination]));
    expect(bySource["/api/:path*"]).toBe("http://localhost:7676/api/:path*");
  });
});
