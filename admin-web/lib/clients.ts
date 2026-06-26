// Project-scoped OIDC login-client endpoints (BFF). An "Application" is a
// Zitadel project; its login clients live at /api/projects/{pid}/apps.
export const clientsBase = (projectId: string): string =>
  `/api/projects/${encodeURIComponent(projectId)}/apps`;

export const clientPath = (projectId: string, appId: string): string =>
  `${clientsBase(projectId)}/${encodeURIComponent(appId)}`;

export const clientSecretPath = (projectId: string, appId: string): string =>
  `${clientPath(projectId, appId)}/secret`;
