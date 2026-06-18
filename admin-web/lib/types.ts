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

// ---- Multi-application authorization (design 2026-06-18, P1) ----
// An "application" is a Zitadel Project with its own roles. Named AppProject to
// avoid colliding with the home-project `Project` settings type above.
export interface AppProject {
  id: string;
  name?: string;
  state?: string;
}
export interface AppProjectList {
  result: AppProject[];
}

// A grant on an application's roster: who can use the app + as which roles.
// Zitadel returns `id` or `grantId` depending on the search/get shape.
export interface ProjectGrant {
  id?: string;
  grantId?: string;
  userId?: string;
  userName?: string;
  displayName?: string;
  roleKeys?: string[];
}
export interface ProjectGrantList {
  result: ProjectGrant[];
}

// ---- Machine-user credentials (service-account JSON keys, design §8) ----
// A key as LISTED — metadata only; the private key is never in the list.
export interface MachineKey {
  id?: string;            // the keyId
  type?: string;          // e.g. KEY_TYPE_JSON
  expirationDate?: string;
  creationDate?: string;
}
export interface MachineKeyList { result: MachineKey[]; }
// The create response: `keyDetails` is base64 of the serviceaccount JSON file,
// returned ONCE. Decode it to produce the <user>-key.json the clients consume.
export interface CreateKeyResponse {
  id?: string;
  keyId?: string;
  keyDetails: string;
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
// The platform org as read from GET /api/org. Read-only in the Console: renaming
// requires ORG_OWNER and is done out-of-band with the runbook.
export interface Org { id: string; name?: string; }
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

// ---- Sessions / status (BFF aggregates) ----
// GET /api/status — operator identity, session expiry, platform health and
// capability flags in one round-trip.
export interface Status {
  operator: { userId: string; name: string; roles: string[] };
  session: { expiresAt: string | null };
  health: { zitadel: boolean };
  capabilities: { events: boolean; chatSessions: boolean };
}

// GET /api/chat-sessions — live chat sessions proxied from the manager's
// control endpoint. `configured=false` when MANAGER_CONTROL_URL is unset.
// A live /chat client as tracked by the manager — carries the session's
// AUTHENTICATED owner (JWT sub).
export interface ChatClient {
  connectionId: string;
  sid: string;
  userId: string;
  backendPort: number;
  connectedAt: string;
  lastQAt?: string | null;
  questionsSent: number;
}

export interface ChatSessions {
  configured: boolean;
  ok?: boolean;
  error?: string;
  list?: {
    count?: number;
    sessions?: string[];
    byBackend?: Record<string, unknown>;
  };
  instances?: {
    ports?: number[];
    sessionsPerPort?: Record<string, number>;
  };
  clients?: {
    ok?: boolean;
    count?: number;
    clients?: ChatClient[];
    error?: string;
  };
}

// GET /api/signins — recent sign-in events (requires the audit capability).
export interface SigninList {
  available: boolean;
  result: AuditEvent[];
}
