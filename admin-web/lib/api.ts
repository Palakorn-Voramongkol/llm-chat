import type { SandboxTemplate, SaveTemplateInput, SaveTemplateResult } from "./types";

export class ApiError extends Error {
  readonly name = "ApiError";
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

async function request<T>(
  path: string,
  init: RequestInit & { json?: unknown } = {},
): Promise<T> {
  const { json, headers, ...rest } = init;
  const res = await fetch(path, {
    ...rest,
    credentials: "include",
    headers: {
      ...(json !== undefined ? { "Content-Type": "application/json" } : {}),
      ...(headers as Record<string, string> | undefined),
    },
    ...(json !== undefined ? { body: JSON.stringify(json) } : {}),
  });

  if (!res.ok) {
    let code = "Error";
    let message = res.statusText || `HTTP ${res.status}`;
    try {
      const body = (await res.json()) as { code?: string; message?: string };
      if (body.code) code = body.code;
      if (body.message) message = body.message;
    } catch {
      /* non-JSON body: keep status text */
    }
    // BFF says "no session" -> the login flow is a full-page nav, not fetch (appendix §5.2)
    if (res.status === 401 && typeof window !== "undefined") {
      window.location.assign("/login");
    }
    throw new ApiError(res.status, code, message);
  }

  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export const api = {
  get: <T>(path: string) => request<T>(path, { method: "GET" }),
  post: <T>(path: string, json?: unknown) =>
    request<T>(path, { method: "POST", json }),
  patch: <T>(path: string, json?: unknown) =>
    request<T>(path, { method: "PATCH", json }),
  put: <T>(path: string, json?: unknown) =>
    request<T>(path, { method: "PUT", json }),
  del: <T>(path: string) => request<T>(path, { method: "DELETE" }),
  getSandboxTemplate: (pid: string, appId: string) =>
    request<SandboxTemplate>(`/api/projects/${pid}/apps/${appId}/sandbox-template`, { method: "GET" }),
  saveSandboxTemplate: (pid: string, appId: string, body: SaveTemplateInput) =>
    request<SaveTemplateResult>(`/api/projects/${pid}/apps/${appId}/sandbox-template`, { method: "PUT", json: body }),
};
