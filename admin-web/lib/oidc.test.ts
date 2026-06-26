import { describe, it, expect } from "vitest";
import { resolveTokenTypes, DEFAULT_GRANT_TYPES, DEFAULT_RESPONSE_TYPES } from "./oidc";
import type { OidcApp } from "@/lib/types";

describe("resolveTokenTypes", () => {
  it("seeds defaults when creating (no existing client)", () => {
    expect(resolveTokenTypes(null)).toEqual({
      responseTypes: DEFAULT_RESPONSE_TYPES,
      grantTypes: DEFAULT_GRANT_TYPES,
    });
  });

  it("preserves the existing client's non-default grant/response types on edit", () => {
    const app: OidcApp = {
      id: "a1",
      name: "portal",
      oidcConfig: {
        clientId: "c1",
        redirectUris: ["https://x/cb"],
        responseTypes: ["OIDC_RESPONSE_TYPE_ID_TOKEN_TOKEN"],
        grantTypes: ["OIDC_GRANT_TYPE_DEVICE_CODE", "OIDC_GRANT_TYPE_TOKEN_EXCHANGE"],
        appType: "OIDC_APP_TYPE_WEB",
        authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC",
      },
    };
    expect(resolveTokenTypes(app)).toEqual({
      responseTypes: ["OIDC_RESPONSE_TYPE_ID_TOKEN_TOKEN"],
      grantTypes: ["OIDC_GRANT_TYPE_DEVICE_CODE", "OIDC_GRANT_TYPE_TOKEN_EXCHANGE"],
    });
  });

  it("returns fresh array copies on both the create and edit paths", () => {
    // create path: not the shared default constants
    const r = resolveTokenTypes(null);
    expect(r.grantTypes).not.toBe(DEFAULT_GRANT_TYPES);
    expect(r.responseTypes).not.toBe(DEFAULT_RESPONSE_TYPES);

    // edit path: not the source client's array instances, but the same contents
    const app: OidcApp = {
      id: "a1",
      name: "portal",
      oidcConfig: {
        clientId: "c1",
        grantTypes: ["OIDC_GRANT_TYPE_DEVICE_CODE"],
        responseTypes: ["OIDC_RESPONSE_TYPE_CODE"],
      },
    };
    const e = resolveTokenTypes(app);
    expect(e.grantTypes).not.toBe(app.oidcConfig!.grantTypes);
    expect(e.responseTypes).not.toBe(app.oidcConfig!.responseTypes);
    expect(e.grantTypes).toEqual(app.oidcConfig!.grantTypes);
    expect(e.responseTypes).toEqual(app.oidcConfig!.responseTypes);
  });
});
