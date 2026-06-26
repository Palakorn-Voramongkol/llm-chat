import type {
  OidcApp, OidcAppType, OidcAuthMethod, OidcGrantType, OidcResponseType,
  CreateOidcAppInput,
} from "@/lib/types";

export interface ConfigForm {
  name: string;
  redirectUris: string;            // newline-separated in the textarea
  responseTypes: OidcResponseType[];
  grantTypes: OidcGrantType[];
  appType: OidcAppType;
  authMethodType: OidcAuthMethod;
}

export const APP_TYPES: { value: OidcAppType; label: string }[] = [
  { value: "OIDC_APP_TYPE_WEB", label: "Web (confidential)" },
  { value: "OIDC_APP_TYPE_NATIVE", label: "Native (PKCE, public)" },
  { value: "OIDC_APP_TYPE_USER_AGENT", label: "User-agent (SPA)" },
];

export const AUTH_METHODS: { value: OidcAuthMethod; label: string }[] = [
  { value: "OIDC_AUTH_METHOD_TYPE_BASIC", label: "Basic (client secret)" },
  { value: "OIDC_AUTH_METHOD_TYPE_POST", label: "POST (client secret)" },
  { value: "OIDC_AUTH_METHOD_TYPE_NONE", label: "None (PKCE only)" },
  { value: "OIDC_AUTH_METHOD_TYPE_PRIVATE_KEY_JWT", label: "Private key JWT" },
];

export const RESPONSE_TYPES: { value: OidcResponseType; label: string }[] = [
  { value: "OIDC_RESPONSE_TYPE_CODE", label: "code" },
  { value: "OIDC_RESPONSE_TYPE_ID_TOKEN", label: "id_token" },
  { value: "OIDC_RESPONSE_TYPE_ID_TOKEN_TOKEN", label: "id_token token" },
];

export const GRANT_TYPES: { value: OidcGrantType; label: string }[] = [
  { value: "OIDC_GRANT_TYPE_AUTHORIZATION_CODE", label: "authorization_code" },
  { value: "OIDC_GRANT_TYPE_REFRESH_TOKEN", label: "refresh_token" },
  { value: "OIDC_GRANT_TYPE_IMPLICIT", label: "implicit" },
  { value: "OIDC_GRANT_TYPE_DEVICE_CODE", label: "device_code" },
  { value: "OIDC_GRANT_TYPE_TOKEN_EXCHANGE", label: "token_exchange" },
];

/// Flatten an app's oidcConfig into the editable form shape (read side of RMW).
export function appToConfigForm(app: OidcApp): ConfigForm {
  const c = app.oidcConfig ?? {};
  return {
    name: app.name,
    redirectUris: (c.redirectUris ?? []).join("\n"),
    responseTypes: c.responseTypes ?? ["OIDC_RESPONSE_TYPE_CODE"],
    grantTypes: c.grantTypes ?? ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE"],
    appType: c.appType ?? "OIDC_APP_TYPE_WEB",
    authMethodType: c.authMethodType ?? "OIDC_AUTH_METHOD_TYPE_BASIC",
  };
}

/// Build the create/update body from the form (write side of RMW).
export function formToConfigBody(f: ConfigForm): CreateOidcAppInput {
  return {
    name: f.name,
    redirectUris: f.redirectUris
      .split("\n").map((s) => s.trim()).filter((s) => s.length > 0),
    responseTypes: f.responseTypes,
    grantTypes: f.grantTypes,
    appType: f.appType,
    authMethodType: f.authMethodType,
  };
}

// The dialog form does not expose response/grant types. On create, seed sensible
// defaults; on EDIT, preserve the client's existing types (the read side of RMW)
// so editing a redirect URI never silently clobbers e.g. device_code / token_exchange.
export const DEFAULT_RESPONSE_TYPES: OidcResponseType[] = ["OIDC_RESPONSE_TYPE_CODE"];
export const DEFAULT_GRANT_TYPES: OidcGrantType[] = [
  "OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN",
];

export function resolveTokenTypes(
  existing: OidcApp | null,
): { responseTypes: OidcResponseType[]; grantTypes: OidcGrantType[] } {
  if (existing) {
    const f = appToConfigForm(existing);
    // Copy so callers can never mutate the source client's arrays (symmetry
    // with the create path below).
    return { responseTypes: [...f.responseTypes], grantTypes: [...f.grantTypes] };
  }
  return {
    responseTypes: [...DEFAULT_RESPONSE_TYPES],
    grantTypes: [...DEFAULT_GRANT_TYPES],
  };
}
