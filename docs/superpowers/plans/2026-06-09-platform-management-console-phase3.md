# Phase 3 (revised: least-privilege, read-only policies)

> This file SUPERSEDES the Phase 3 section of `2026-06-09-platform-management-console.md`.
> Per the security review (spec §3/§5/§9), the runtime SA is least-privilege
> (`ORG_USER_MANAGER` + `PROJECT_OWNER`): the project is editable, but org
> policies are **read-only** (GET only; a 403 degrades to "Managed out-of-band").
> There is NO policy-write path and NO ADMIN_IT upsert task.

## Phase 3: Project & Org Settings (read-only policies)

Mirrors Phase 2 (Applications) across the three reused layers (`zitadel/*.rs`
method → `Operator`-gated handler in `api/mod.rs` → typed `lib/types.ts` → thin
`(dash)` page + cards). Differences enforced by the security review:

- **Project** is editable: `GET /api/project` + `PUT /api/project` (the SA holds
  `PROJECT_OWNER`).
- **Org policies** (login, password-complexity, lockout) are **read-only**: GET
  only, rendered in read-only cards. No PUT/upsert, no ADMIN_IT error-code task,
  no `mfaInitSkipLifetime` editing (provisioner-managed now). A policy GET that
  403s must NOT error the page — the client method tolerates `Forbidden` by
  returning a typed "unavailable", and the card degrades to "Managed
  out-of-band" (capability-style, like Audit §11).

**NAV note:** `admin-web/components/shell/nav.ts` already defines the entry as
`{ label: "Project & Org", href: "/settings", match: "/settings" }` (Phase 0).
Do NOT add a duplicate or rename it. The page lives at
`admin-web/app/(dash)/settings/page.tsx` (URL `/settings`); the e2e navigates to
`/settings`.

**Verified Zitadel endpoints (exact paths):**

| Console route | Zitadel call | Writable? |
|---|---|---|
| `GET /api/project` | `GET /management/v1/projects/{id}` | read |
| `PUT /api/project` | `PUT /management/v1/projects/{id}` | ✅ PROJECT_OWNER |
| `GET /api/org/policies/login` | `GET /management/v1/policies/login` | ❌ read-only |
| `GET /api/org/policies/password-complexity` | `GET /management/v1/policies/password/complexity` | ❌ read-only |
| `GET /api/org/policies/lockout` | `GET /management/v1/policies/lockout` | ❌ read-only |

**Env (every task):** backend `cargo build/test -p llm-chat-admin-api` (crate is
`llm-chat-admin-api`); frontend pnpm is `corepack pnpm@9.15.9 <cmd>`. Tailwind v4
gradients are `bg-linear-*`. Next 16 `(dash)` adds no URL segment. Do NOT run
Playwright/e2e or any live-Zitadel step — controller runs e2e at phase end.
Commit messages end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

Before writing any Rust, READ the real files you mirror: `admin-api/src/zitadel/apps.rs`
(method style + `get_json`/`put_json` helpers), `admin-api/src/zitadel/error.rs`
(confirm the EXACT 403 variant name — the examples below assume `ZitadelError::Forbidden`;
if it differs, use the real name), `admin-api/src/api/mod.rs` (Operator handlers +
camelCase request structs + the `contract_tests` mod), `admin-api/src/zitadel/mod.rs`.
For frontend mirror `admin-web/components/apps/*`, `admin-web/app/(dash)/apps/page.tsx`,
`admin-web/lib/{types,api}.ts`, `admin-web/components/ui/{card,switch,form}.tsx`.

---

### Task 3.1: zitadel/project.rs — get_project + update_project

**Files:**
- Create: `admin-api/src/zitadel/project.rs`
- Modify: `admin-api/src/zitadel/mod.rs` (add `pub mod project;`)

- [ ] **Step 1 — failing test.** Create `project.rs` with ONLY a pure body-builder + its test (no `impl` yet) so it fails on the missing module wiring:
```rust
//! Project read + update within the llm-chat project (design §9). The SA holds
//! PROJECT_OWNER, so GET + PUT here are within least privilege. v1 Management
//! API. Mirrors zitadel/apps.rs method style.
use serde_json::{json, Value};
use super::error::ZitadelError;
use super::ZitadelClient;

/// PURE: the PUT /projects/{id} body (read-modify-write the whole settings set).
fn update_project_body(name: &str, role_assertion: bool, role_check: bool, has_project_check: bool) -> Value {
    json!({ "name": name, "projectRoleAssertion": role_assertion,
            "projectRoleCheck": role_check, "hasProjectCheck": has_project_check })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_project_body_carries_name_and_settings() {
        let b = update_project_body("llm-chat", true, false, true);
        assert_eq!(b["name"], "llm-chat");
        assert_eq!(b["projectRoleAssertion"], true);
        assert_eq!(b["projectRoleCheck"], false);
        assert_eq!(b["hasProjectCheck"], true);
    }
}
```
- [ ] **Step 2 — run, expect FAIL** (module not declared): `cargo test -p llm-chat-admin-api project::tests::update_project_body_carries_name_and_settings`
- [ ] **Step 3 — implement.** Add `pub mod project;` to `zitadel/mod.rs` (keep alphabetical), then add the impl between the pure fn and the test mod:
```rust
impl ZitadelClient {
    /// GET /management/v1/projects/{id} — returns the project entity (unwrapped
    /// from its { "project": {...} } envelope; falls back to the whole value).
    pub async fn get_project(&self) -> Result<Value, ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}", self.cfg.issuer, pid);
        let v = self.get_json(&url).await?;
        Ok(v.get("project").cloned().unwrap_or(v))
    }
    /// PUT /management/v1/projects/{id} — PROJECT_OWNER covers this.
    pub async fn update_project(&self, name: &str, role_assertion: bool, role_check: bool, has_project_check: bool) -> Result<(), ZitadelError> {
        let pid = &self.cfg.project_id;
        let url = format!("{}/management/v1/projects/{}", self.cfg.issuer, pid);
        let body = update_project_body(name, role_assertion, role_check, has_project_check);
        self.put_json(&url, &body).await.map(|_| ())
    }
}
```
(If the real helper isn't `put_json`/`get_json`, use whatever `apps.rs` uses.)
- [ ] **Step 4 — run, expect PASS:** `cargo test -p llm-chat-admin-api project::tests` then `cargo build -p llm-chat-admin-api`
- [ ] **Step 5 — commit:** `git add admin-api/src/zitadel/project.rs admin-api/src/zitadel/mod.rs` then commit `feat(admin-api): zitadel project get/update (PROJECT_OWNER, design §9)`.

---

### Task 3.2: zitadel/policies.rs — three read-only getters that tolerate 403

**Files:**
- Create: `admin-api/src/zitadel/policies.rs`
- Modify: `admin-api/src/zitadel/mod.rs` (add `pub mod policies;`)

- [ ] **Step 1 — failing test.** Create `policies.rs` with the typed enum + a pure classifier + tests:
```rust
//! Read-only org-policy getters (design §9). The least-privilege SA may not even
//! be able to READ org policies, so a 403 degrades to a typed "unavailable";
//! every OTHER error propagates. No write path by design.
use serde_json::Value;
use super::error::ZitadelError;
use super::ZitadelClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyRead { Available(Value), Unavailable }

/// PURE: a 403 (Forbidden) → Unavailable; unwrap the { "policy": {...} }
/// envelope; every other error propagates. (Confirm the real 403 variant name
/// in zitadel/error.rs; replace ZitadelError::Forbidden if it differs.)
fn classify_policy(res: Result<Value, ZitadelError>) -> Result<PolicyRead, ZitadelError> {
    match res {
        Ok(v) => Ok(PolicyRead::Available(v.get("policy").cloned().unwrap_or(v))),
        Err(ZitadelError::Forbidden) => Ok(PolicyRead::Unavailable),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test] fn available_unwraps_envelope() {
        assert_eq!(classify_policy(Ok(json!({"policy":{"minLength":"8"}}))).unwrap(),
                   PolicyRead::Available(json!({"minLength":"8"})));
    }
    #[test] fn forbidden_degrades() {
        assert_eq!(classify_policy(Err(ZitadelError::Forbidden)).unwrap(), PolicyRead::Unavailable);
    }
    #[test] fn other_errors_propagate() {
        assert!(classify_policy(Err(ZitadelError::NotFound)).is_err());
    }
}
```
- [ ] **Step 2 — run, expect FAIL:** `cargo test -p llm-chat-admin-api policies::tests`
- [ ] **Step 3 — implement.** Add `pub mod policies;` to `zitadel/mod.rs`, then the impl:
```rust
impl ZitadelClient {
    pub async fn get_login_policy(&self) -> Result<PolicyRead, ZitadelError> {
        let url = format!("{}/management/v1/policies/login", self.cfg.issuer);
        classify_policy(self.get_json(&url).await)
    }
    pub async fn get_password_complexity_policy(&self) -> Result<PolicyRead, ZitadelError> {
        let url = format!("{}/management/v1/policies/password/complexity", self.cfg.issuer);
        classify_policy(self.get_json(&url).await)
    }
    pub async fn get_lockout_policy(&self) -> Result<PolicyRead, ZitadelError> {
        let url = format!("{}/management/v1/policies/lockout", self.cfg.issuer);
        classify_policy(self.get_json(&url).await)
    }
}
```
- [ ] **Step 4 — run, expect PASS:** `cargo test -p llm-chat-admin-api policies::tests` then `cargo build -p llm-chat-admin-api`
- [ ] **Step 5 — commit:** `git add admin-api/src/zitadel/policies.rs admin-api/src/zitadel/mod.rs` then commit `feat(admin-api): read-only org-policy getters tolerating 403 (design §9)`.

---

### Task 3.3: api/mod.rs — routes + handlers + camelCase contract tests

**Files:**
- Modify: `admin-api/src/api/mod.rs`

- [ ] **Step 1 — failing test.** Add to the `contract_tests` mod:
```rust
    #[test]
    fn update_project_accepts_camelcase() {
        let b: UpdateProject = serde_json::from_value(json!({
            "name":"llm-chat","projectRoleAssertion":true,"projectRoleCheck":false,"hasProjectCheck":true
        })).expect("camelCase UpdateProject");
        assert_eq!(b.name, "llm-chat");
        assert!(b.project_role_assertion);
        assert!(!b.project_role_check);
        assert!(b.has_project_check);
    }
```
- [ ] **Step 2 — run, expect FAIL** (`UpdateProject` undefined): `cargo test -p llm-chat-admin-api --lib contract_tests::update_project_accepts_camelcase`
- [ ] **Step 3 — implement.** Add routes after the `/api/apps/{appId}/secret` route:
```rust
        .route("/api/project", get(get_project).put(update_project))
        .route("/api/org/policies/login", get(get_login_policy))
        .route("/api/org/policies/password-complexity", get(get_password_complexity_policy))
        .route("/api/org/policies/lockout", get(get_lockout_policy))
```
Then add the struct + handlers (near the app handlers, before `mod contract_tests`):
```rust
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProject {
    name: String,
    #[serde(default)] project_role_assertion: bool,
    #[serde(default)] project_role_check: bool,
    #[serde(default)] has_project_check: bool,
}

async fn get_project(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(st.zitadel.get_project().await?))
}
async fn update_project(_op: Operator, State(st): State<AppState>, Json(b): Json<UpdateProject>) -> Result<Json<Value>, ApiError> {
    st.zitadel.update_project(&b.name, b.project_role_assertion, b.project_role_check, b.has_project_check).await?;
    Ok(Json(json!({ "ok": true })))
}

// Read-only policy handlers: Unavailable (degraded 403) surfaces as a 200
// envelope { available:false, policy:null }, never an HTTP error (design §9).
fn policy_envelope(p: crate::zitadel::policies::PolicyRead) -> Json<Value> {
    use crate::zitadel::policies::PolicyRead;
    match p {
        PolicyRead::Available(v) => Json(json!({ "available": true, "policy": v })),
        PolicyRead::Unavailable => Json(json!({ "available": false, "policy": Value::Null })),
    }
}
async fn get_login_policy(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(policy_envelope(st.zitadel.get_login_policy().await?))
}
async fn get_password_complexity_policy(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(policy_envelope(st.zitadel.get_password_complexity_policy().await?))
}
async fn get_lockout_policy(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(policy_envelope(st.zitadel.get_lockout_policy().await?))
}
```
(Match the real handler signature/imports used by the existing app handlers — `Operator`, `State<AppState>`, `ApiError`, `Json`, `Value`, `json!`.)
- [ ] **Step 4 — run, expect PASS:** `cargo test -p llm-chat-admin-api --lib contract_tests::update_project_accepts_camelcase` then `cargo build -p llm-chat-admin-api`
- [ ] **Step 5 — commit:** `git add admin-api/src/api/mod.rs` then commit `feat(admin-api): /api/project (GET+PUT) + read-only /api/org/policies/* (design §9)`.

---

### Task 3.4: lib/types.ts — Project + read-only policy types

**Files:**
- Modify: `admin-web/lib/types.ts`
- Create: `admin-web/lib/types.project.test-d.ts` (compile-time contract)

- [ ] **Step 1 — failing typecheck.** Create `types.project.test-d.ts` importing the not-yet-existing types and asserting their shape (Duration fields are STRINGS):
```ts
import type { Project, LoginPolicy, PasswordComplexityPolicy, LockoutPolicy, PolicyEnvelope } from "@/lib/types";
const p: Project = { id:"x", name:"llm-chat", projectRoleAssertion:true, projectRoleCheck:false, hasProjectCheck:true }; void p;
const login: PolicyEnvelope<LoginPolicy> = { available:true, policy:{ allowUsernamePassword:true, forceMfa:false, mfaInitSkipLifetime:"0s" } }; void login;
const pw: PasswordComplexityPolicy = { minLength:"8", hasUppercase:true, hasLowercase:true, hasNumber:true, hasSymbol:false }; void pw;
const lock: LockoutPolicy = { maxPasswordAttempts:"5" }; void lock;
const degraded: PolicyEnvelope<LockoutPolicy> = { available:false, policy:null }; void degraded;
```
- [ ] **Step 2 — run, expect FAIL:** `corepack pnpm@9.15.9 exec tsc --noEmit`
- [ ] **Step 3 — implement.** Append to `lib/types.ts`:
```ts
// ---- Project & Org settings (design §9) ----
export interface Project { id: string; name: string; projectRoleAssertion?: boolean; projectRoleCheck?: boolean; hasProjectCheck?: boolean; }
export interface UpdateProjectInput { name: string; projectRoleAssertion: boolean; projectRoleCheck: boolean; hasProjectCheck: boolean; }
// Org policies are READ-ONLY (design §9): no Update*Policy type. Duration fields
// are protobuf STRINGS ("240h0m0s","0s"), typed string.
export interface LoginPolicy { allowUsernamePassword?: boolean; allowRegister?: boolean; allowExternalIdp?: boolean; forceMfa?: boolean; passwordlessType?: string; mfaInitSkipLifetime?: string; }
export interface PasswordComplexityPolicy { minLength?: string; hasUppercase?: boolean; hasLowercase?: boolean; hasNumber?: boolean; hasSymbol?: boolean; }
export interface LockoutPolicy { maxPasswordAttempts?: string; }
export interface PolicyEnvelope<T> { available: boolean; policy: T | null; }
```
- [ ] **Step 4 — run, expect PASS:** `corepack pnpm@9.15.9 exec tsc --noEmit`
- [ ] **Step 5 — commit:** `git add admin-web/lib/types.ts admin-web/lib/types.project.test-d.ts` then commit `feat(admin-web): Project + read-only policy types (design §9)`.

---

### Task 3.5: components/project/* — editable Project card + read-only policy cards

**Files:**
- Create: `admin-web/components/project/project-card.tsx` (editable)
- Create: `admin-web/components/project/policy-card.tsx` (read-only display + degrade)

- [ ] **Step 1 — failing typecheck.** Create `admin-web/components/project/policy-card.test-d.ts`:
```ts
import type { ComponentProps } from "react";
import { PolicyCard } from "@/components/project/policy-card";
const props: ComponentProps<typeof PolicyCard> = { title:"Login policy", description:"x", available:true, rows:[{label:"Force MFA", value:"no"}] }; void props;
const degraded: ComponentProps<typeof PolicyCard> = { title:"Lockout policy", description:"", available:false, rows:[] }; void degraded;
```
- [ ] **Step 2 — run, expect FAIL:** `corepack pnpm@9.15.9 exec tsc --noEmit`
- [ ] **Step 3 — implement `policy-card.tsx`** (read-only; the degrade note lives here). Use `Card*` from `@/components/ui/card`, `Badge` from `@/components/ui/badge`. `data-testid={`policy-card-${title.toLowerCase().replace(/\s+/g,"-")}`}`, a "Read-only" `<Badge variant="secondary">`, a `<dl>` of rows when `available`, else a `<p data-testid="policy-managed-out-of-band">Managed out-of-band…</p>`.
- [ ] **Step 3b — implement `project-card.tsx`** (editable; mirror `components/apps/app-form-dialog.tsx`'s react-hook-form + zodResolver + `api.put("/api/project", values)` + `toast`/`ApiError`). Fields: `name` (`Input`, testid `project-name`), three `Switch` toggles (projectRoleAssertion/projectRoleCheck/hasProjectCheck), a Save button (testid `project-save`, disabled until `project` loads). `useEffect` resets the form when `project` arrives. If `@/components/ui/form` does NOT export `FormDescription`, use a plain `<p className="text-muted-foreground text-sm">` instead (read the real form.tsx; do not invent an export).
- [ ] **Step 4 — run, expect PASS:** `corepack pnpm@9.15.9 exec tsc --noEmit`
- [ ] **Step 5 — commit:** `git add admin-web/components/project/` then commit `feat(admin-web): editable Project card + read-only policy cards (design §9)`.

---

### Task 3.6: app/(dash)/settings/page.tsx — the Project & Org page

**Files:**
- Create: `admin-web/app/(dash)/settings/page.tsx`

- [ ] **Step 1.** Read `node_modules/next/dist/docs/` routing guidance (per admin-web/AGENTS.md) + mirror `app/(dash)/apps/page.tsx`.
- [ ] **Step 2 — implement.** `'use client'`; a `useCallback load()` that fetches `/api/project` into state and EACH policy envelope independently with its own try/catch degrade to `{available:false,policy:null}` (one failure never blanks others, §9/§12); `api.get("/api/me").catch(()=>{})` on mount. Render an `<h1>Project &amp; Org</h1>` + subtitle (policies read-only / provisioner-managed), `<ProjectCard project onSaved={load} />`, and three `<PolicyCard>`s (login/password-complexity/lockout) built from the fetched policies, formatting booleans as yes/no and Duration/count strings as-is (`"0s"`,`"240h0m0s"`,`"8"`, with `—` fallback). Titles EXACTLY: "Login policy", "Password complexity", "Lockout policy".
- [ ] **Step 3 — run, expect PASS:** `corepack pnpm@9.15.9 exec tsc --noEmit` then `corepack pnpm@9.15.9 build` (confirms the `/settings` route compiles).
- [ ] **Step 4 — commit:** `git add "admin-web/app/(dash)/settings/page.tsx"` then commit `feat(admin-web): Project & Org settings page (/settings) — editable project + read-only policies (design §9)`.

---

### Task 3.7: e2e — Settings renders editable project + read-only policies (WRITE, do NOT run)

**Files:**
- Modify: `admin-web/e2e/smoke.spec.ts` (append a test inside the `authenticated operator flow` describe)

- [ ] **Step 1 — write the spec** (do NOT run; controller runs e2e at phase end):
```ts
  test("Project & Org settings renders editable project + read-only policies", async ({ page }) => {
    await page.goto("/settings"); // NAV href is /settings (nav.ts), not /project
    await expect(page.getByRole("heading", { name: "Project & Org" })).toBeVisible();
    await expect(page.getByTestId("project-card")).toBeVisible();
    await expect(page.getByTestId("project-name")).toBeVisible();
    await expect(page.getByTestId("project-save")).toBeVisible();
    const loginCard = page.getByTestId("policy-card-login-policy");
    await expect(loginCard).toBeVisible();
    await expect(loginCard.getByText("Read-only")).toBeVisible();
    await expect(page.getByTestId("policy-card-password-complexity")).toBeVisible();
    await expect(page.getByTestId("policy-card-lockout-policy")).toBeVisible();
    await expect(page.getByTestId("project-save")).toHaveCount(1); // only the project is mutable
  });
```
- [ ] **Step 2 — verify offline only:** `npx playwright test smoke --list` enumerates the new title; `corepack pnpm@9.15.9 exec tsc --noEmit` clean. Do NOT start the stack.
- [ ] **Step 3 — commit:** `git add admin-web/e2e/smoke.spec.ts` then commit `test(admin-web): e2e Settings — editable project + read-only policy cards (design §9)`.

**Exit criteria:** `cargo test -p llm-chat-admin-api` green (new `project::`/`policies::`/`contract_tests::update_project_*`); `tsc --noEmit` + build clean; `/settings` shows an editable Project card + three read-only policy cards, any unreadable policy degrading to "Managed out-of-band". No org-policy write path exists anywhere — the least-privilege boundary (§3/§5/§9) is preserved.
