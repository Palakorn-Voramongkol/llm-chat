import { describe, it, expect } from "vitest";
import { NAV } from "./nav";

describe("nav", () => {
  it("has no standalone OIDC Clients page", () => {
    expect(NAV.find((n) => n.href === "/apps")).toBeUndefined();
    expect(NAV.map((n) => n.label)).not.toContain("OIDC Clients");
  });
  it("keeps the Applications page", () => {
    expect(NAV.find((n) => n.href === "/applications")).toBeDefined();
  });
});
