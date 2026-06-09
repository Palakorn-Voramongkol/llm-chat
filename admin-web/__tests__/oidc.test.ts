import { describe, it, expect } from "vitest";
import { appToConfigForm, formToConfigBody, APP_TYPES, AUTH_METHODS } from "../lib/oidc";
import type { OidcApp } from "../lib/types";

const app: OidcApp = {
  id: "a1",
  name: "Chat",
  oidcConfig: {
    clientId: "c1",
    redirectUris: ["https://x/cb", "https://x/cb2"],
    responseTypes: ["OIDC_RESPONSE_TYPE_CODE"],
    grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
    appType: "OIDC_APP_TYPE_WEB",
    authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC",
  },
};

describe("oidc mapper", () => {
  it("appToConfigForm flattens uris/types to newline + comma strings", () => {
    const f = appToConfigForm(app);
    expect(f.redirectUris).toBe("https://x/cb\nhttps://x/cb2");
    expect(f.appType).toBe("OIDC_APP_TYPE_WEB");
    expect(f.authMethodType).toBe("OIDC_AUTH_METHOD_TYPE_BASIC");
    expect(f.grantTypes).toEqual(
      ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"],
    );
  });

  it("formToConfigBody splits + trims + drops blank redirect lines", () => {
    const body = formToConfigBody({
      name: "Chat",
      redirectUris: "  https://x/cb \n\n https://x/cb2 \n",
      responseTypes: ["OIDC_RESPONSE_TYPE_CODE"],
      grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
      appType: "OIDC_APP_TYPE_NATIVE",
      authMethodType: "OIDC_AUTH_METHOD_TYPE_NONE",
    });
    expect(body.redirectUris).toEqual(["https://x/cb", "https://x/cb2"]);
    expect(body.appType).toBe("OIDC_APP_TYPE_NATIVE");
  });

  it("exposes the enum option lists", () => {
    expect(APP_TYPES.map((o) => o.value)).toContain("OIDC_APP_TYPE_WEB");
    expect(AUTH_METHODS.map((o) => o.value)).toContain("OIDC_AUTH_METHOD_TYPE_NONE");
  });
});
