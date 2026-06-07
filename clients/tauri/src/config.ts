// Product identity + frontend-visible defaults. The Rust shell is the source of
// truth for network/auth config (it reads LUMINA_* env vars); these are used for
// display and as fallbacks only.
export const APP_NAME = "Lumina";

// Authorization (beyond authentication): the account must hold this project role
// to use the app. Checked from the JWT after login by the AuthorizationGate.
export const REQUIRED_ROLE = "chat.app";

export const DEFAULTS = {
  managerWs: "ws://127.0.0.1:7777/chat",
  issuer: "http://host.docker.internal:8080",
};
