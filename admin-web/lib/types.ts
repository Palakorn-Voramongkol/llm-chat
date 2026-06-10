export type UserKind = "Human" | "Machine";
export type UserState =
  | "ACTIVE" | "INACTIVE" | "LOCKED" | "INITIAL" | "DELETED" | "UNSPECIFIED";

export interface User {
  id: string;
  userName: string;
  kind: UserKind;
  state: UserState;
  email?: string;
  displayName?: string;
}

export interface UserList {
  result: User[];
  total?: number;
}

export interface Me {
  userId: string;
  name: string;
  roles: string[];
}

export interface Role {
  key: string;
  displayName: string;
  group?: string;
}

export interface UserGrant {
  grantId: string;
  projectId: string;
  roleKeys: string[];
}

export interface CreateHumanInput {
  userName: string;
  givenName: string;
  familyName: string;
  email: string;
  password?: string;
}

export interface CreateMachineInput {
  userName: string;
  name: string;
}

export interface EditProfileInput {
  givenName: string;
  familyName: string;
  displayName?: string;
}

export interface RoleList {
  result: Role[];
}

export interface RoleHolder {
  id: string;
  userId: string;
  roleKeys: string[];
  displayName?: string;
  userName?: string;
}

export interface RoleHolderList {
  result: RoleHolder[];
}

export interface GrantList {
  result: UserGrant[];
}

export type OidcAppType = "OIDC_APP_TYPE_WEB" | "OIDC_APP_TYPE_NATIVE" | "OIDC_APP_TYPE_USER_AGENT";
export type OidcAuthMethod =
  | "OIDC_AUTH_METHOD_TYPE_BASIC" | "OIDC_AUTH_METHOD_TYPE_POST"
  | "OIDC_AUTH_METHOD_TYPE_NONE" | "OIDC_AUTH_METHOD_TYPE_PRIVATE_KEY_JWT";
export type OidcResponseType = "OIDC_RESPONSE_TYPE_CODE" | "OIDC_RESPONSE_TYPE_ID_TOKEN" | "OIDC_RESPONSE_TYPE_ID_TOKEN_TOKEN";
export type OidcGrantType =
  | "OIDC_GRANT_TYPE_AUTHORIZATION_CODE" | "OIDC_GRANT_TYPE_IMPLICIT"
  | "OIDC_GRANT_TYPE_REFRESH_TOKEN" | "OIDC_GRANT_TYPE_DEVICE_CODE" | "OIDC_GRANT_TYPE_TOKEN_EXCHANGE";

export interface OidcConfig {
  clientId?: string;
  redirectUris?: string[];
  responseTypes?: OidcResponseType[];
  grantTypes?: OidcGrantType[];
  appType?: OidcAppType;
  authMethodType?: OidcAuthMethod;
}

export interface OidcApp {
  id: string;
  name: string;
  state?: string;
  oidcConfig?: OidcConfig;
}

export interface OidcAppList {
  result: OidcApp[];
}

export interface CreateOidcAppInput {
  name: string;
  redirectUris: string[];
  responseTypes: OidcResponseType[];
  grantTypes: OidcGrantType[];
  appType: OidcAppType;
  authMethodType: OidcAuthMethod;
}

// Returned ONCE on create + secret regenerate; never readable again.
export interface AppSecret {
  appId?: string;
  clientId?: string;
  clientSecret: string;
}

// ---- Project & Org settings (design §9) ----
export interface Project { id: string; name: string; projectRoleAssertion?: boolean; projectRoleCheck?: boolean; hasProjectCheck?: boolean; }
export interface UpdateProjectInput { name: string; projectRoleAssertion: boolean; projectRoleCheck: boolean; hasProjectCheck: boolean; }
// Org policies are READ-ONLY (design §9): no Update*Policy type. Duration fields
// are protobuf STRINGS ("240h0m0s","0s"), typed string.
export interface LoginPolicy { allowUsernamePassword?: boolean; allowRegister?: boolean; allowExternalIdp?: boolean; forceMfa?: boolean; passwordlessType?: string; mfaInitSkipLifetime?: string; }
export interface PasswordComplexityPolicy { minLength?: string; hasUppercase?: boolean; hasLowercase?: boolean; hasNumber?: boolean; hasSymbol?: boolean; }
export interface LockoutPolicy { maxPasswordAttempts?: string; }
export interface PolicyEnvelope<T> { available: boolean; policy: T | null; }

// ---- Dashboard stats (design §10) ----
// camelCase mirror of the /api/stats BFF JSON. Counts are number|null: a null
// count means that sub-query failed/degraded and renders as an em-dash (—).
export interface Stats {
  humans: number | null;
  machines: number | null;
  roles: number | null;
  grants: number | null;
  apps: number | null;
  tokenHealthy: boolean;
}

// ---- Audit / event log (design §11) ----
// Whether the service account can read the org event log (requires
// IAM_OWNER_VIEWER). When false the audit page fails closed and shows a banner.
export interface Capabilities {
  events: boolean;
}

// camelCase passthrough from /admin/v1/events/_search — editor, aggregate,
// type, creationDate are the Zitadel event fields.
// A localized Zitadel enum: { type, localized: { localizedMessage } }.
interface ZitadelLocalized {
  type?: string;
  localized?: { localizedMessage?: string };
}

export interface AuditEvent {
  sequence?: string;
  creationDate?: string;
  type?: ZitadelLocalized;
  // System events have only `service` (e.g. "zitadel"); user actions carry userId/displayName.
  editor?: { userId?: string; displayName?: string; service?: string };
  // NOTE: aggregate.type is an OBJECT (localized enum), not a string.
  aggregate?: { id?: string; type?: ZitadelLocalized; resourceOwner?: string };
}

export interface EventList {
  result: AuditEvent[];
}
