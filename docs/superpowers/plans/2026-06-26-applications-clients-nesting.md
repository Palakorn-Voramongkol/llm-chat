# Applications ↔ OIDC Login Clients Nesting — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make OIDC login clients a managed child of each Application (Zitadel project): manage them on the application detail page, retire the standalone "OIDC Clients" page, and add project-scoped client CRUD to the BFF.

**Architecture:** admin-api gains project-scoped OIDC-client CRUD endpoints (`/api/projects/{pid}/apps…`) mirroring the existing project-scoped *list*; admin-web rebuilds `/applications/[id]` as a master–detail (client list + detail panel + create/edit/rotate/delete) with Roles & Users as secondary cards, adds a Clients count to the Applications list, removes the nav item, and redirects `/apps` into the home application's detail.

**Tech Stack:** Rust (axum) + `serde_json` (admin-api); Next.js 16 / React 19 + TypeScript + shadcn/ui + TanStack Table + vitest (admin-web). Spec: `docs/superpowers/specs/2026-06-26-applications-clients-nesting-design.md`.

## Global Constraints

- **Fail-closed, no fallback.** Client CRUD on a project the SA doesn't own returns Zitadel 403 → mapped to HTTP 403; surface it as a `toast.error`, never fall back to the home project or swallow it.
- **Every new admin-api route takes the `Operator` extractor** (chat.admin gate). No exceptions.
- **Path-traversal is already blocked centrally** in `zitadel/mod.rs::send_json` (`path_has_traversal`); do not add or relax it.
- **One-time secret reveal invariant:** `clientSecret` from create/regenerate is returned once, streamed straight through, never logged or refetched.
- **Keep the home-project aliases** (`/api/apps`, `/api/apps/{appId}`, `/api/apps/{appId}/secret`) working; only the frontend stops using them.
- **No project Edit/Delete in the app header** — project create/rename/delete is the `new_app.py` provisioner runbook, not a Console power.
- **shadcn-first (admin-web `AGENTS.md`):** compose existing `components/ui/*` primitives; do not hand-roll styled `<button>`/`<input>`. Use only `Button` variants/sizes defined in `components/ui/button.tsx` (observed in repo: variants `brand`, `ghost`, `default`, `outline`; sizes `sm`, `icon-sm`) — verify a variant exists before using it.
- **Copy:** use the term **"login client"** (not "application") for OIDC clients in the new UI.
- **admin-api crate name:** `llm-chat-admin-api`. **admin-web package manager:** `pnpm` (run from repo root with `-C admin-web`).
- **Commit convention:** Conventional Commits; end every commit message body with the trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Stage explicit paths only (shared branch — never `git add -A`).

---

## File Structure

**admin-api**
- `admin-api/src/zitadel/apps.rs` — *modify*: add 5 `*_in(project_id, …)` client-CRUD methods; turn the 5 existing home-project methods into thin aliases.
- `admin-api/src/api/mod.rs` — *modify*: add 5 project-scoped routes + handlers; add a gating test.

**admin-web**
- `admin-web/lib/clients.ts` — *create*: pure URL builders for the project-scoped client endpoints.
- `admin-web/lib/clients.test.ts` — *create*: unit tests for the builders.
- `admin-web/components/apps/app-form-dialog.tsx` — *modify*: add `projectId` prop; project-scoped endpoints; "login client" copy.
- `admin-web/components/apps/app-form-dialog.test.tsx` — *create*: render test for the create-mode trigger copy.
- `admin-web/app/(dash)/applications/[id]/page.tsx` — *modify (rewrite)*: master–detail clients + CRUD + Roles/Users cards.
- `admin-web/components/applications/columns.tsx` — *modify*: add `clientCount` to `AppMeta` + a **Clients** column.
- `admin-web/components/applications/columns.test.tsx` — *create*: assert the Clients column exists.
- `admin-web/app/(dash)/applications/page.tsx` — *modify*: fetch each app's client count into `AppMeta`.
- `admin-web/components/shell/nav.ts` — *modify*: remove the "OIDC Clients" entry.
- `admin-web/components/shell/nav.test.ts` — *create*: assert `/apps` is no longer a nav page.
- `admin-web/app/(dash)/apps/page.tsx` — *modify (rewrite)*: redirect to the home application's detail.

---

### Task 1: Project-scope the OIDC-client CRUD (admin-api `zitadel/apps.rs`)

**Files:**
- Modify: `admin-api/src/zitadel/apps.rs`

**Interfaces:**
- Consumes: existing `ZitadelClient` HTTP helpers `post_json`, `get_json`, `put_json`, `delete`; pure body builders `oidc_create_body`, `oidc_update_body`; `self.cfg.project_id`.
- Produces: `create_oidc_app_in(&self, pid, name, redirect_uris, response_types, grant_types, app_type, auth_method) -> Result<Value, ZitadelError>`; `get_app_in(&self, pid, app_id) -> Result<Value, ZitadelError>`; `update_oidc_config_in(&self, pid, app_id, redirect_uris, response_types, grant_types, app_type, auth_method) -> Result<(), ZitadelError>`; `regenerate_app_secret_in(&self, pid, app_id) -> Result<Value, ZitadelError>`; `delete_app_in(&self, pid, app_id) -> Result<(), ZitadelError>`. The existing same-named methods (without `_in`) remain, delegating to these with `&self.cfg.project_id`.

- [ ] **Step 1: Replace the `impl ZitadelClient` block body** in `admin-api/src/zitadel/apps.rs` (the block currently spanning the `list_apps … delete_app` methods, lines ~61–139). Keep the two pure `fn oidc_create_body` / `fn oidc_update_body` above it unchanged. New block:

```rust
impl ZitadelClient {
    /// List the HOME project's apps (§8). Thin alias over `list_apps_for`.
    pub async fn list_apps(&self) -> Result<Vec<Value>, ZitadelError> {
        let pid = self.cfg.project_id.clone();
        self.list_apps_for(&pid).await
    }

    /// List ANY project's apps (login clients): POST
    /// /management/v1/projects/{pid}/apps/_search.
    pub async fn list_apps_for(&self, project_id: &str) -> Result<Vec<Value>, ZitadelError> {
        let url = format!("{}/management/v1/projects/{}/apps/_search", self.cfg.issuer, project_id);
        let v = self.post_json(&url, &json!({})).await?;
        Ok(v.get("result").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    /// Create an OIDC app in ANY project. Returns the FULL response —
    /// clientId + clientSecret (shown ONCE) live here; never logged.
    pub async fn create_oidc_app_in(
        &self,
        project_id: &str,
        name: &str,
        redirect_uris: &[String],
        response_types: &[String],
        grant_types: &[String],
        app_type: &str,
        auth_method: &str,
    ) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/projects/{}/apps/oidc", self.cfg.issuer, project_id);
        let body = oidc_create_body(name, redirect_uris, response_types, grant_types, app_type, auth_method);
        self.post_json(&url, &body).await
    }

    /// Create an OIDC app in the HOME project. Thin alias.
    pub async fn create_oidc_app(
        &self,
        name: &str,
        redirect_uris: &[String],
        response_types: &[String],
        grant_types: &[String],
        app_type: &str,
        auth_method: &str,
    ) -> Result<Value, ZitadelError> {
        let pid = self.cfg.project_id.clone();
        self.create_oidc_app_in(&pid, name, redirect_uris, response_types, grant_types, app_type, auth_method).await
    }

    /// Get one app in ANY project.
    pub async fn get_app_in(&self, project_id: &str, app_id: &str) -> Result<Value, ZitadelError> {
        let url = format!("{}/management/v1/projects/{}/apps/{}", self.cfg.issuer, project_id, app_id);
        self.get_json(&url).await
    }

    /// Get one app in the HOME project. Thin alias.
    pub async fn get_app(&self, app_id: &str) -> Result<Value, ZitadelError> {
        let pid = self.cfg.project_id.clone();
        self.get_app_in(&pid, app_id).await
    }

    /// Replace an app's whole oidc_config in ANY project.
    pub async fn update_oidc_config_in(
        &self,
        project_id: &str,
        app_id: &str,
        redirect_uris: &[String],
        response_types: &[String],
        grant_types: &[String],
        app_type: &str,
        auth_method: &str,
    ) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/projects/{}/apps/{}/oidc_config", self.cfg.issuer, project_id, app_id);
        let body = oidc_update_body(redirect_uris, response_types, grant_types, app_type, auth_method);
        self.put_json(&url, &body).await.map(|_| ())
    }

    /// Replace an app's whole oidc_config in the HOME project. Thin alias.
    pub async fn update_oidc_config(
        &self,
        app_id: &str,
        redirect_uris: &[String],
        response_types: &[String],
        grant_types: &[String],
        app_type: &str,
        auth_method: &str,
    ) -> Result<(), ZitadelError> {
        let pid = self.cfg.project_id.clone();
        self.update_oidc_config_in(&pid, app_id, redirect_uris, response_types, grant_types, app_type, auth_method).await
    }

    /// Regenerate the client secret in ANY project. Returns clientSecret ONCE.
    pub async fn regenerate_app_secret_in(&self, project_id: &str, app_id: &str) -> Result<Value, ZitadelError> {
        let url = format!(
            "{}/management/v1/projects/{}/apps/{}/oidc_config/_generate_client_secret",
            self.cfg.issuer, project_id, app_id
        );
        self.post_json(&url, &json!({})).await
    }

    /// Regenerate the client secret in the HOME project. Thin alias.
    pub async fn regenerate_app_secret(&self, app_id: &str) -> Result<Value, ZitadelError> {
        let pid = self.cfg.project_id.clone();
        self.regenerate_app_secret_in(&pid, app_id).await
    }

    /// Delete an app in ANY project.
    pub async fn delete_app_in(&self, project_id: &str, app_id: &str) -> Result<(), ZitadelError> {
        let url = format!("{}/management/v1/projects/{}/apps/{}", self.cfg.issuer, project_id, app_id);
        self.delete(&url).await.map(|_| ())
    }

    /// Delete an app in the HOME project. Thin alias.
    pub async fn delete_app(&self, app_id: &str) -> Result<(), ZitadelError> {
        let pid = self.cfg.project_id.clone();
        self.delete_app_in(&pid, app_id).await
    }
}
```

- [ ] **Step 2: Verify it compiles and the existing pure tests still pass**

Run: `cargo test -p llm-chat-admin-api --lib zitadel::apps`
Expected: PASS — `oidc_create_body_carries_the_provisioner_proven_fields` and `oidc_update_body_omits_name_but_keeps_the_full_config` still pass; the crate compiles (the home-project methods are unchanged in signature, so `api/mod.rs` callers still build).

- [ ] **Step 3: Commit**

```bash
git add admin-api/src/zitadel/apps.rs
git commit -F - <<'EOF'
feat(admin-api): project-scope OIDC client CRUD methods

Add create/get/update/regenerate-secret/delete *_in(pid, ...) variants
mirroring list_apps_for; keep the home-project methods as thin aliases.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: Project-scoped client routes (admin-api `api/mod.rs`)

**Files:**
- Modify: `admin-api/src/api/mod.rs` (router ~line 60; handlers near the multi-application section ~line 561; gating test in `mod contract_tests`)

**Interfaces:**
- Consumes: `Task 1` methods `create_oidc_app_in`, `get_app_in`, `update_oidc_config_in`, `regenerate_app_secret_in`, `delete_app_in`; existing request structs `CreateOidcApp`, `UpdateOidcConfig`; `Operator` extractor; `test_router_no_session()` test helper.
- Produces: routes `POST /api/projects/{pid}/apps`, `GET|PUT|DELETE /api/projects/{pid}/apps/{appId}`, `POST /api/projects/{pid}/apps/{appId}/secret`.

- [ ] **Step 1: Write the failing gating test.** Add to `mod contract_tests` at the bottom of `admin-api/src/api/mod.rs`:

```rust
    #[tokio::test]
    async fn project_app_routes_require_operator() {
        use tower::ServiceExt;
        let cases: &[(&str, &str)] = &[
            ("POST", "/api/projects/p1/apps"),
            ("GET", "/api/projects/p1/apps/a1"),
            ("PUT", "/api/projects/p1/apps/a1"),
            ("DELETE", "/api/projects/p1/apps/a1"),
            ("POST", "/api/projects/p1/apps/a1/secret"),
        ];
        for (method, uri) in cases {
            let app = test_router_no_session();
            let res = app
                .oneshot(
                    axum::http::Request::builder()
                        .method(*method)
                        .uri(*uri)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                axum::http::StatusCode::UNAUTHORIZED,
                "{method} {uri} must be Operator-gated (401), got {}",
                res.status()
            );
        }
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p llm-chat-admin-api --lib api::contract_tests::project_app_routes_require_operator`
Expected: FAIL — routes don't exist yet, so unauthenticated requests get `404 NOT_FOUND`, not `401`.

- [ ] **Step 3: Add the routes.** In `pub fn router`, immediately after the existing `.route("/api/projects/{pid}/apps", get(list_project_apps))` line, add:

```rust
        .route("/api/projects/{pid}/apps", get(list_project_apps).post(create_project_app))
        .route("/api/projects/{pid}/apps/{appId}", get(get_project_app).put(update_project_app).delete(delete_project_app))
        .route("/api/projects/{pid}/apps/{appId}/secret", post(regenerate_project_app_secret))
```

Then **delete** the now-duplicated original `.route("/api/projects/{pid}/apps", get(list_project_apps))` line (the new `.route(...get().post())` replaces it — a duplicate path with `get(...)` only would otherwise be registered twice).

- [ ] **Step 4: Add the handlers.** After `list_project_apps` (~line 564), add:

```rust
async fn create_project_app(_op: Operator, State(st): State<AppState>, Path(pid): Path<String>, Json(b): Json<CreateOidcApp>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.create_oidc_app_in(
        &pid, &b.name, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?))
}

async fn get_project_app(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_app_in(&pid, &app_id).await?))
}

async fn update_project_app(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>, Json(b): Json<UpdateOidcConfig>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.update_oidc_config_in(
        &pid, &app_id, &b.redirect_uris, &b.response_types, &b.grant_types, &b.app_type, &b.auth_method_type,
    ).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_project_app(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    st.zitadel.delete_app_in(&pid, &app_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn regenerate_project_app_secret(_op: Operator, State(st): State<AppState>, Path((pid, app_id)): Path<(String, String)>)
    -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.regenerate_app_secret_in(&pid, &app_id).await?))
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p llm-chat-admin-api --lib api::contract_tests::project_app_routes_require_operator`
Expected: PASS — all 5 routes now return `401` without a session.

- [ ] **Step 6: Run the full admin-api test suite**

Run: `cargo test -p llm-chat-admin-api`
Expected: PASS (no regressions).

- [ ] **Step 7: Commit**

```bash
git add admin-api/src/api/mod.rs
git commit -F - <<'EOF'
feat(admin-api): project-scoped OIDC client routes

POST /api/projects/{pid}/apps, GET|PUT|DELETE .../{appId}, and
POST .../{appId}/secret, all Operator-gated. Gating test added.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: Client endpoint URL builders (admin-web `lib/clients.ts`)

**Files:**
- Create: `admin-web/lib/clients.ts`
- Test: `admin-web/lib/clients.test.ts`

**Interfaces:**
- Produces: `clientsBase(projectId: string): string`, `clientPath(projectId: string, appId: string): string`, `clientSecretPath(projectId: string, appId: string): string`.

- [ ] **Step 1: Write the failing test.** Create `admin-web/lib/clients.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { clientsBase, clientPath, clientSecretPath } from "./clients";

describe("client endpoint builders", () => {
  it("builds the project-scoped base", () => {
    expect(clientsBase("370")).toBe("/api/projects/370/apps");
  });
  it("builds a single-client path", () => {
    expect(clientPath("370", "abc")).toBe("/api/projects/370/apps/abc");
  });
  it("builds a secret path", () => {
    expect(clientSecretPath("370", "abc")).toBe("/api/projects/370/apps/abc/secret");
  });
  it("URL-encodes the ids", () => {
    expect(clientsBase("a b")).toBe("/api/projects/a%20b/apps");
    expect(clientPath("p", "a/b")).toBe("/api/projects/p/apps/a%2Fb");
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm -C admin-web exec vitest run lib/clients.test.ts`
Expected: FAIL — `./clients` cannot be resolved.

- [ ] **Step 3: Write the implementation.** Create `admin-web/lib/clients.ts`:

```ts
// Project-scoped OIDC login-client endpoints (BFF). An "Application" is a
// Zitadel project; its login clients live at /api/projects/{pid}/apps.
export const clientsBase = (projectId: string): string =>
  `/api/projects/${encodeURIComponent(projectId)}/apps`;

export const clientPath = (projectId: string, appId: string): string =>
  `${clientsBase(projectId)}/${encodeURIComponent(appId)}`;

export const clientSecretPath = (projectId: string, appId: string): string =>
  `${clientPath(projectId, appId)}/secret`;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `pnpm -C admin-web exec vitest run lib/clients.test.ts`
Expected: PASS (4 assertions).

- [ ] **Step 5: Commit**

```bash
git add admin-web/lib/clients.ts admin-web/lib/clients.test.ts
git commit -F - <<'EOF'
feat(admin-web): project-scoped client endpoint builders

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 4: Parametrize `AppFormDialog` by project (admin-web)

**Files:**
- Modify: `admin-web/components/apps/app-form-dialog.tsx`
- Test: `admin-web/components/apps/app-form-dialog.test.tsx`

**Interfaces:**
- Consumes: `Task 3` `clientsBase`, `clientPath`; existing `lib/oidc.ts` mappers; `lib/api`.
- Produces: `AppFormDialog` now requires a `projectId: string` prop; create posts to `clientsBase(projectId)`, edit PUTs to `clientPath(projectId, app.id)`. Create-mode trigger copy is **"Register login client"**.

- [ ] **Step 1: Write the failing test.** Create `admin-web/components/apps/app-form-dialog.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { AppFormDialog } from "./app-form-dialog";

vi.mock("@/lib/api", () => ({
  api: { post: vi.fn(), put: vi.fn() },
  ApiError: class ApiError extends Error {},
}));

describe("AppFormDialog", () => {
  it("renders the 'Register login client' trigger in create mode", () => {
    render(
      <AppFormDialog
        mode="create"
        projectId="370"
        app={null}
        open={false}
        onOpenChange={() => {}}
        onSaved={() => {}}
        onSecret={() => {}}
      />,
    );
    expect(screen.getByTestId("create-app").textContent).toContain("Register login client");
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm -C admin-web exec vitest run components/apps/app-form-dialog.test.tsx`
Expected: FAIL — either a TypeScript error (`projectId` not a prop) or the trigger text is still "Create application".

- [ ] **Step 3: Add the `projectId` prop and use the builders.** In `admin-web/components/apps/app-form-dialog.tsx`:

3a. Add the import after the existing `lib/oidc` import:

```tsx
import { clientsBase, clientPath } from "@/lib/clients";
```

3b. Add `projectId` to the destructured props and the prop type:

```tsx
export function AppFormDialog({
  mode, projectId, app, open, onOpenChange, onSaved, onSecret,
}: {
  mode: "create" | "edit";
  projectId: string;
  app: OidcApp | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
  onSecret: (s: AppSecret) => void;
}) {
```

3c. In `onSubmit`, replace the two endpoints. Change the edit branch `await api.put(`/api/apps/${app.id}`, {…})` to:

```tsx
        await api.put(clientPath(projectId, app.id), {
          redirectUris: body.redirectUris,
          responseTypes: body.responseTypes,
          grantTypes: body.grantTypes,
          appType: body.appType,
          authMethodType: body.authMethodType,
        });
        toast.success("Login client updated");
```

and the create branch `const created = await api.post<AppSecret>("/api/apps", body);` to:

```tsx
        const created = await api.post<AppSecret>(clientsBase(projectId), body);
        toast.success("Login client created");
```

3d. Update the dialog title and trigger copy. Change the `DialogTitle` to:

```tsx
      <DialogHeader><DialogTitle>{isEdit ? "Edit login client" : "Register login client"}</DialogTitle></DialogHeader>
```

and the create-mode trigger button label:

```tsx
        <Button variant="brand" data-testid="create-app">Register login client</Button>
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `pnpm -C admin-web exec vitest run components/apps/app-form-dialog.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add admin-web/components/apps/app-form-dialog.tsx admin-web/components/apps/app-form-dialog.test.tsx
git commit -F - <<'EOF'
feat(admin-web): scope AppFormDialog to a project

Add a required projectId prop; create/edit now target
/api/projects/{pid}/apps. Relabel to "login client".

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 5: Rebuild the Application detail page as master–detail with client CRUD (admin-web)

**Files:**
- Modify (rewrite): `admin-web/app/(dash)/applications/[id]/page.tsx`

**Interfaces:**
- Consumes: `Task 3` `clientPath`, `clientSecretPath`; `Task 4` `AppFormDialog` (now needs `projectId`); existing `SecretRevealDialog`, `ConfirmDialog`, `RoleCreateDialog`, `RoleEditDialog`, `DetailPanel`/`PanelSection`/`PanelField`, `appTypeLabel`/`pretty`, `PageHeader`, `Card*`, `Button`, `avatarGradient`/`initials`; existing project-scoped role + grants endpoints; new `GET /api/projects/{id}/apps` list.
- Produces: the page (no exported symbols other than the default component).

- [ ] **Step 1: Confirm reusable component props before writing** (no guessing). Run these reads and verify:

Run: `pnpm -C admin-web exec tsc --noEmit` is the final gate, but first confirm by reading:
- `admin-web/components/ui/detail-panel.tsx` — `DetailPanel` accepts `open`, `title`, `subtitle`, `onClose`, children; `PanelSection` accepts `title`; `PanelField` accepts `label`, `mono`.
- `admin-web/components/ui/button.tsx` — `variant="outline"` and `size="sm"` are defined (verified: variants `default`/`outline`/`secondary`/`ghost`/`destructive`/`link`/`brand`, sizes `sm`/`icon-sm`/…). Use them as written.
- `admin-web/components/apps/secret-reveal-dialog.tsx` — `SecretRevealDialog` accepts `clientId`, `clientSecret`, `onClose`.

Expected: the prop names above match (they are used verbatim in the current `app/(dash)/apps/page.tsx`).

- [ ] **Step 2: Replace the entire file** `admin-web/app/(dash)/applications/[id]/page.tsx` with:

```tsx
"use client";
import { useCallback, useEffect, useState } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { ArrowLeft, ShieldCheck, AppWindow, Pencil, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/shell/PageHeader";
import { DetailPanel, PanelField, PanelSection } from "@/components/ui/detail-panel";
import { ConfirmDialog } from "@/components/users/confirm-dialog";
import { RoleCreateDialog } from "@/components/applications/role-create-dialog";
import { RoleEditDialog } from "@/components/roles/role-edit-dialog";
import { AppFormDialog } from "@/components/apps/app-form-dialog";
import { SecretRevealDialog } from "@/components/apps/secret-reveal-dialog";
import { appTypeLabel, pretty } from "@/components/apps/columns";
import { avatarGradient, initials } from "@/lib/avatar";
import { api, ApiError } from "@/lib/api";
import { clientPath, clientSecretPath } from "@/lib/clients";
import type {
  AppProject, AppProjectList, OidcApp, OidcAppList,
  ProjectGrant, ProjectGrantList, Role, RoleList, AppSecret,
} from "@/lib/types";

const APP_TYPE_CHIP: Record<string, string> = {
  NATIVE: "bg-emerald-500/10 text-emerald-700",
  WEB: "bg-blue-500/10 text-blue-700",
  API: "bg-violet-500/10 text-violet-700",
  USER_AGENT: "bg-amber-500/10 text-amber-700",
};

export default function ApplicationDetailPage() {
  const params = useParams<{ id: string }>();
  const id = params.id;
  const [project, setProject] = useState<AppProject | null>(null);
  const [roles, setRoles] = useState<Role[]>([]);
  const [clients, setClients] = useState<OidcApp[]>([]);
  const [grants, setGrants] = useState<ProjectGrant[]>([]);
  const [selected, setSelected] = useState<OidcApp | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<OidcApp | null>(null);
  const [rotateTarget, setRotateTarget] = useState<OidcApp | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<OidcApp | null>(null);
  const [revealed, setRevealed] = useState<AppSecret | null>(null);
  const [deleteRole, setDeleteRole] = useState<Role | null>(null);
  const [editRole, setEditRole] = useState<Role | null>(null);

  const loadRoles = useCallback(async () => {
    if (!id) return;
    try {
      const rl = await api.get<RoleList>(`/api/projects/${id}/roles`);
      setRoles(rl.result ?? []);
    } catch { setRoles([]); }
  }, [id]);

  const loadClients = useCallback(async () => {
    if (!id) return;
    try {
      const al = await api.get<OidcAppList>(`/api/projects/${id}/apps`);
      setClients(al.result ?? []);
    } catch { setClients([]); }
  }, [id]);

  const load = useCallback(async () => {
    if (!id) return;
    try {
      const list = await api.get<AppProjectList>("/api/projects");
      setProject((list.result ?? []).find((p) => p.id === id) ?? null);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load application");
      }
    }
    await Promise.all([
      loadRoles(),
      loadClients(),
      (async () => {
        try {
          const gl = await api.get<ProjectGrantList>(`/api/projects/${id}/grants`);
          setGrants(gl.result ?? []);
        } catch { setGrants([]); }
      })(),
    ]);
  }, [id, loadRoles, loadClients]);

  useEffect(() => { load(); }, [load]);

  async function confirmRotate() {
    if (!rotateTarget) return;
    const target = rotateTarget;
    setRotateTarget(null);
    try {
      const s = await api.post<AppSecret>(clientSecretPath(id, target.id));
      toast.success("Secret rotated");
      if (s?.clientSecret) setRevealed(s); // one-time reveal
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Rotate failed");
    }
  }

  async function confirmDeleteClient() {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    try {
      await api.del(clientPath(id, target.id));
      toast.success("Login client deleted");
      if (selected?.id === target.id) setSelected(null);
      loadClients();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Delete failed");
    }
  }

  async function confirmDeleteRole() {
    if (!id || !deleteRole) return;
    const key = deleteRole.key;
    setDeleteRole(null);
    try {
      await api.del(`/api/projects/${id}/roles/${encodeURIComponent(key)}`);
      toast.success("Role removed");
      loadRoles();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Remove failed");
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4 px-6 py-6 overflow-auto">
      <div className="space-y-2">
        <Link href="/applications"
          className="text-muted-foreground hover:text-foreground inline-flex items-center gap-1.5 text-sm">
          <ArrowLeft className="size-4" />
          Applications
        </Link>
        <PageHeader title={project?.name || id}
          description="Login clients, roles, and who can use this application." />
      </div>

      {/* Login clients — master/detail (primary surface). */}
      <div className="flex min-h-0 gap-4">
        <div className="min-w-0 flex-1">
          <Card>
            <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
              <CardTitle className="text-base">Login clients</CardTitle>
              <AppFormDialog mode="create" projectId={id} app={null}
                open={createOpen} onOpenChange={setCreateOpen}
                onSaved={loadClients} onSecret={setRevealed} />
            </CardHeader>
            <CardContent>
              {clients.length === 0 ? (
                <p className="text-muted-foreground text-sm">No login clients.</p>
              ) : (
                <ul className="space-y-2.5">
                  {clients.map((c) => {
                    const t = appTypeLabel(c);
                    const isSel = selected?.id === c.id;
                    return (
                      <li key={c.id}>
                        <button type="button" onClick={() => setSelected(c)}
                          className={`flex w-full items-center gap-2.5 rounded-md border px-2.5 py-2 text-left transition-colors ${isSel ? "border-primary ring-1 ring-primary" : "hover:bg-muted/50"}`}>
                          <span aria-hidden
                            className="flex size-7 shrink-0 items-center justify-center rounded-md bg-violet-500/10 text-violet-600">
                            <AppWindow className="size-4" />
                          </span>
                          <span className="flex min-w-0 flex-col">
                            <span className="truncate text-sm font-medium">{c.name}</span>
                            <span className="text-muted-foreground truncate font-mono text-xs">
                              {c.oidcConfig?.clientId || "—"}
                            </span>
                          </span>
                          {t && (
                            <span className={`ml-auto inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${APP_TYPE_CHIP[t] ?? "bg-slate-500/10 text-slate-600"}`}>
                              {t}
                            </span>
                          )}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </CardContent>
          </Card>
        </div>

        <DetailPanel open={!!selected} title={selected?.name ?? ""}
          subtitle="Login client" onClose={() => setSelected(null)}>
          {selected && (
            <>
              <PanelSection title="OIDC config">
                <PanelField label="Client ID" mono>{selected.oidcConfig?.clientId || "—"}</PanelField>
                <PanelField label="App type">{appTypeLabel(selected) || "—"}</PanelField>
                <PanelField label="Auth method">{pretty(selected.oidcConfig?.authMethodType) || "—"}</PanelField>
                <PanelField label="Grant types">
                  {selected.oidcConfig?.grantTypes?.length
                    ? selected.oidcConfig.grantTypes.map(pretty).join(", ")
                    : "—"}
                </PanelField>
              </PanelSection>
              <PanelSection title="Redirect URIs">
                {selected.oidcConfig?.redirectUris?.length ? (
                  <ul className="space-y-1">
                    {selected.oidcConfig.redirectUris.map((uri) => (
                      <li key={uri} className="font-mono text-xs break-all">{uri}</li>
                    ))}
                  </ul>
                ) : (
                  <span className="text-muted-foreground text-sm">—</span>
                )}
              </PanelSection>
              <div className="flex flex-wrap gap-2 pt-2">
                <Button variant="outline" size="sm" onClick={() => setEditTarget(selected)}>Edit</Button>
                <Button variant="outline" size="sm" onClick={() => setRotateTarget(selected)}>Rotate secret</Button>
                <Button variant="outline" size="sm" className="text-destructive"
                  onClick={() => setDeleteTarget(selected)}>Delete</Button>
              </div>
            </>
          )}
        </DetailPanel>
      </div>

      {/* Roles + Users (secondary). */}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-2 space-y-0">
            <CardTitle className="text-base">Roles</CardTitle>
            <RoleCreateDialog projectId={id} onCreated={loadRoles} />
          </CardHeader>
          <CardContent>
            {roles.length === 0 ? (
              <p className="text-muted-foreground text-sm">No roles defined.</p>
            ) : (
              <ul className="space-y-2.5">
                {roles.map((r) => (
                  <li key={r.key} className="flex items-start gap-2.5">
                    <span aria-hidden
                      className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-md bg-indigo-500/10 text-indigo-600">
                      <ShieldCheck className="size-4" />
                    </span>
                    <span className="flex min-w-0 flex-col">
                      <span className="font-mono text-sm font-medium">{r.key}</span>
                      <span className="text-muted-foreground truncate text-xs">{r.displayName || "—"}</span>
                    </span>
                    {r.group && (
                      <span className="ml-auto inline-flex items-center rounded-full bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
                        {r.group}
                      </span>
                    )}
                    <Button variant="ghost" size="icon-sm"
                      className={`text-muted-foreground hover:text-foreground shrink-0${r.group ? " ml-1.5" : " ml-auto"}`}
                      data-testid="app-role-edit" aria-label={`Edit role ${r.key}`}
                      onClick={() => setEditRole(r)}>
                      <Pencil className="size-4" />
                    </Button>
                    <Button variant="ghost" size="icon-sm"
                      className="text-muted-foreground hover:text-destructive ml-0.5 shrink-0"
                      data-testid="app-role-delete" aria-label={`Delete role ${r.key}`}
                      onClick={() => setDeleteRole(r)}>
                      <Trash2 className="size-4" />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Users</CardTitle>
          </CardHeader>
          <CardContent>
            {grants.length === 0 ? (
              <p className="text-muted-foreground text-sm">No users have access.</p>
            ) : (
              <ul className="space-y-3">
                {grants.map((g, i) => {
                  const name = g.displayName || g.userName || g.userId || "—";
                  const seed = g.userId || g.userName || String(i);
                  return (
                    <li key={g.id || g.grantId || seed} className="space-y-1.5">
                      <span className="flex items-center gap-2.5">
                        <span aria-hidden
                          className={`flex size-7 shrink-0 items-center justify-center rounded-full bg-linear-to-br text-[10px] font-bold text-white ${avatarGradient(seed)}`}>
                          {initials(name)}
                        </span>
                        <span className="flex min-w-0 flex-col">
                          <span className="truncate text-sm">{name}</span>
                          {g.userId && (
                            <span className="text-muted-foreground truncate font-mono text-xs">{g.userId}</span>
                          )}
                        </span>
                      </span>
                      <span className="flex flex-wrap gap-1 pl-9">
                        {(g.roleKeys ?? []).length ? (
                          (g.roleKeys ?? []).map((rk) => (
                            <span key={rk}
                              className="inline-flex items-center rounded-md bg-slate-500/10 px-2 py-0.5 text-xs font-medium text-slate-600">
                              {rk}
                            </span>
                          ))
                        ) : (
                          <span className="text-muted-foreground text-xs">—</span>
                        )}
                      </span>
                    </li>
                  );
                })}
              </ul>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Dialogs. */}
      <AppFormDialog mode="edit" projectId={id} app={editTarget} open={!!editTarget}
        onOpenChange={(o) => !o && setEditTarget(null)} onSaved={loadClients} onSecret={setRevealed} />
      <SecretRevealDialog clientId={revealed?.clientId}
        clientSecret={revealed?.clientSecret ?? null} onClose={() => setRevealed(null)} />
      <ConfirmDialog open={!!rotateTarget} onOpenChange={(o) => !o && setRotateTarget(null)}
        title="Rotate client secret?"
        description="A new secret is generated and shown once. Any client still using the old secret will immediately fail authentication until updated."
        confirmLabel="Rotate" onConfirm={confirmRotate} />
      <ConfirmDialog open={!!deleteTarget} onOpenChange={(o) => !o && setDeleteTarget(null)}
        title="Delete login client?"
        description="This removes the OIDC client. Changing or removing it can instantly break a live login for users mid-flow. This cannot be undone."
        confirmLabel="Delete" onConfirm={confirmDeleteClient} />
      <RoleEditDialog role={editRole}
        endpoint={`/api/projects/${id}/roles/${encodeURIComponent(editRole?.key ?? "")}`}
        open={!!editRole} onOpenChange={(o) => !o && setEditRole(null)} onSaved={loadRoles} />
      <ConfirmDialog open={!!deleteRole} onOpenChange={(o) => !o && setDeleteRole(null)}
        title={`Remove role ${deleteRole?.key ?? ""}?`}
        description="This cascades — the role is stripped from every user grant on this application. This cannot be undone."
        confirmLabel="Remove role" onConfirm={confirmDeleteRole} />
    </div>
  );
}
```

- [ ] **Step 3: Typecheck**

Run: `pnpm -C admin-web exec tsc --noEmit`
Expected: PASS — no type errors. (`variant="outline"` and `size="sm"` are defined in `components/ui/button.tsx` — verified.)

- [ ] **Step 4: Run the admin-web test suite (no regressions)**

Run: `pnpm -C admin-web exec vitest run`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add "admin-web/app/(dash)/applications/[id]/page.tsx"
git commit -F - <<'EOF'
feat(admin-web): manage login clients inside the application detail

Rebuild /applications/[id] as a master-detail of login clients (create
/ edit / rotate secret / delete via project-scoped endpoints), with
Roles and Users as secondary cards.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 6: Add a Clients count to the Applications list (admin-web)

**Files:**
- Modify: `admin-web/components/applications/columns.tsx`
- Test: `admin-web/components/applications/columns.test.tsx`
- Modify: `admin-web/app/(dash)/applications/page.tsx`

**Interfaces:**
- Consumes: existing `AppProject`, `OidcAppList`, the page's per-app `Promise.all` fan-out.
- Produces: `AppMeta` gains `clientCount: number`; `buildApplicationColumns` gains a column with `id: "clients"`.

- [ ] **Step 1: Write the failing test.** Create `admin-web/components/applications/columns.test.tsx`:

```tsx
import { describe, it, expect } from "vitest";
import type { ColumnDef } from "@tanstack/react-table";
import type { AppProject } from "@/lib/types";
import { buildApplicationColumns } from "./columns";

const colId = (c: ColumnDef<AppProject>): string | undefined =>
  // tanstack columns carry either an explicit `id` or an `accessorKey`.
  (c as { id?: string }).id ?? (c as { accessorKey?: string }).accessorKey;

describe("application columns", () => {
  it("includes a clients column", () => {
    const ids = buildApplicationColumns(new Map()).map(colId);
    expect(ids).toContain("clients");
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm -C admin-web exec vitest run components/applications/columns.test.tsx`
Expected: FAIL — `expect(ids).toContain("clients")` fails (no such column yet).

- [ ] **Step 3: Add `clientCount` to `AppMeta`.** In `admin-web/components/applications/columns.tsx`, change the interface:

```tsx
export interface AppMeta {
  roleKeys: string[]; // the app's role keys (for the chips)
  userCount: number; // distinct users with a grant on the app
  clientCount: number; // login clients registered under the app
}
```

- [ ] **Step 4: Add the Clients column.** In the same file, insert this column object into the array returned by `buildApplicationColumns`, immediately after the `users` column object (before the closing `]`):

```tsx
    {
      id: "clients", header: "Clients", enableSorting: false,
      meta: { description: "How many OIDC login clients are registered under this application." },
      cell: ({ row }) => {
        const meta = metaById?.get(row.original.id);
        if (!meta) return <span className="text-muted-foreground text-xs">…</span>;
        return <span className="tabular-nums text-sm">{meta.clientCount}</span>;
      },
    },
```

- [ ] **Step 5: Run the column test to verify it passes**

Run: `pnpm -C admin-web exec vitest run components/applications/columns.test.tsx`
Expected: PASS.

- [ ] **Step 6: Fetch the client count on the list page.** In `admin-web/app/(dash)/applications/page.tsx`, update the imports and the per-app `Promise.all` body. Change the type import line to add `OidcAppList`:

```tsx
import type {
  AppProject, AppProjectList, OidcAppList, ProjectGrantList, RoleList,
} from "@/lib/types";
```

Replace the inner `try { … } catch { return null; }` block of the `projects.map(async (p) => …)` with:

```tsx
        try {
          const [roles, grants, apps] = await Promise.all([
            api.get<RoleList>(`/api/projects/${p.id}/roles`),
            api.get<ProjectGrantList>(`/api/projects/${p.id}/grants`),
            api.get<OidcAppList>(`/api/projects/${p.id}/apps`),
          ]);
          const userCount = new Set(
            (grants.result ?? [])
              .map((g) => g.userId)
              .filter((u): u is string => !!u),
          ).size;
          return [p.id, {
            roleKeys: (roles.result ?? []).map((r) => r.key),
            userCount,
            clientCount: (apps.result ?? []).length,
          }];
        } catch {
          return null;
        }
```

- [ ] **Step 7: Typecheck**

Run: `pnpm -C admin-web exec tsc --noEmit`
Expected: PASS — `AppMeta` consumers now supply `clientCount`.

- [ ] **Step 8: Commit**

```bash
git add admin-web/components/applications/columns.tsx admin-web/components/applications/columns.test.tsx "admin-web/app/(dash)/applications/page.tsx"
git commit -F - <<'EOF'
feat(admin-web): show login-client count on the Applications list

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 7: Remove the OIDC Clients nav item and redirect `/apps` (admin-web)

**Files:**
- Modify: `admin-web/components/shell/nav.ts`
- Test: `admin-web/components/shell/nav.test.ts`
- Modify (rewrite): `admin-web/app/(dash)/apps/page.tsx`

**Interfaces:**
- Consumes: existing `GET /api/project` (home project, includes `id`); `Project` type.
- Produces: `NAV` no longer contains a `/apps` entry; `/apps` route redirects to `/applications/{home-pid}`.

- [ ] **Step 1: Write the failing test.** Create `admin-web/components/shell/nav.test.ts`:

```ts
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `pnpm -C admin-web exec vitest run components/shell/nav.test.ts`
Expected: FAIL — the `/apps` "OIDC Clients" entry is still present.

- [ ] **Step 3: Remove the nav entry.** In `admin-web/components/shell/nav.ts`, delete this line from the `NAV` array:

```tsx
  { icon: AppWindow, label: "OIDC Clients", href: "/apps", match: "/apps" },
```

Then remove the now-unused `AppWindow` from the `lucide-react` import on line 2 (keep the other icons).

- [ ] **Step 4: Run the nav test to verify it passes**

Run: `pnpm -C admin-web exec vitest run components/shell/nav.test.ts`
Expected: PASS.

- [ ] **Step 5: Replace `/apps` with a redirect.** Replace the entire file `admin-web/app/(dash)/apps/page.tsx` with:

```tsx
"use client";
import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { api } from "@/lib/api";
import type { Project } from "@/lib/types";

// Legacy route. OIDC login clients are now managed inside each Application
// (/applications/<id>). Redirect to the home application's detail so old
// bookmarks still land on the platform project's clients.
export default function AppsRedirectPage() {
  const router = useRouter();
  useEffect(() => {
    let alive = true;
    api
      .get<Project>("/api/project")
      .then((p) => { if (alive) router.replace(p?.id ? `/applications/${p.id}` : "/applications"); })
      .catch(() => { if (alive) router.replace("/applications"); });
    return () => { alive = false; };
  }, [router]);
  return null;
}
```

- [ ] **Step 6: Typecheck + full test + lint**

Run: `pnpm -C admin-web exec tsc --noEmit && pnpm -C admin-web exec vitest run`
Expected: PASS — no type errors, all unit tests green. (If the repo lints in CI, also run `pnpm -C admin-web exec eslint app components lib` and fix any unused-import warnings.)

- [ ] **Step 7: Commit**

```bash
git add admin-web/components/shell/nav.ts admin-web/components/shell/nav.test.ts "admin-web/app/(dash)/apps/page.tsx"
git commit -F - <<'EOF'
feat(admin-web): retire standalone OIDC Clients page

Remove the nav item; redirect /apps to the home application's detail
where its login clients are now managed.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 8: Update the Playwright shell e2e for the nav change (admin-web)

**Files:**
- Modify: `admin-web/e2e/shell.spec.ts`

**Interfaces:**
- Consumes: nothing new. This task only aligns the gated e2e suite with the removed nav item.

- [ ] **Step 1: Read the current shell spec.**

Run: open `admin-web/e2e/shell.spec.ts` and find any assertion referencing the "OIDC Clients" label or the `/apps` href (e.g. a nav-renders-all-items test or a navigation click).

- [ ] **Step 2: Update the assertions.** Remove the "OIDC Clients" / `/apps` expectations. If a test navigates to `/apps`, change it to assert the redirect lands on an `/applications/...` URL, e.g.:

```ts
await page.goto("/apps");
await expect(page).toHaveURL(/\/applications(\/|$)/);
```

If the spec asserts the exact set of nav labels, drop "OIDC Clients" from that expected array.

- [ ] **Step 3: Run the smoke e2e (route-mocked, no stack needed)**

Run: `pnpm -C admin-web exec playwright test e2e/shell.spec.ts`
Expected: PASS for the route-mocked smoke cases. (The full authenticated suite still runs only under `ADMIN_IT=1` against the compose stack — out of scope here.)

- [ ] **Step 4: Commit**

```bash
git add admin-web/e2e/shell.spec.ts
git commit -F - <<'EOF'
test(admin-web): align shell e2e with retired OIDC Clients nav

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
```

---

## Final verification (after all tasks)

- [ ] `cargo test -p llm-chat-admin-api` — PASS.
- [ ] `pnpm -C admin-web exec tsc --noEmit` — PASS.
- [ ] `pnpm -C admin-web exec vitest run` — PASS.
- [ ] Manual (or `ADMIN_IT=1` e2e) against the compose stack: open `/applications`, confirm the **Clients** count column; open an application; create a login client (secret revealed once); edit it; rotate the secret; delete it; confirm `/apps` redirects into the home application's detail; confirm the sidebar no longer shows "OIDC Clients".

## Self-review notes (author)

- **Spec coverage:** backend `_in` methods + aliases (Task 1) and 5 routes (Task 2) ✓; master–detail client CRUD on the detail page (Task 5) ✓; Clients count column (Task 6) ✓; nav removal + `/apps` redirect (Task 7) ✓; e2e nav update (Task 8) ✓; fail-closed 403 surfacing inherited via the shared `api`/`toast.error` path used by every mutation ✓; one-time secret reveal preserved via `SecretRevealDialog` ✓; no project Edit/Delete in header ✓.
- **Out of scope (unchanged):** project create/rename/delete; app-code surfacing; a global all-clients view; chat's project binding.
- **Type consistency:** `clientsBase`/`clientPath`/`clientSecretPath` (Task 3) are the only client URL source and are used identically in Tasks 4–5; `AppMeta.clientCount` (Task 6) is set in the list page's `Promise.all` and read in the new column; new admin-api methods’ signatures match their callers in `api/mod.rs`.
