# Versioned per-app sandbox templates — Console editor + LLM migration (Sub-project 2)

> Design doc. Builds directly on Sub-project 1
> (`docs/superpowers/specs/2026-06-29-per-app-sandbox-templates-design.md`),
> which delivered the manager-owned template store
> (`app_sandbox_template`), `parse_template`/variable substitution, the
> `/provision` path, and the worker's `provision-app-box` (write-if-absent).

## Goal

Let operators **author** a per-app sandbox template in the admin-web Console
(folder tree + per-file content), **version** it, and have each user's sandbox
be **reconciled to the current version on login** — created on first login,
and **migrated by the LLM** when the operator publishes a newer version.

## Scope

One sub-project covering all of: the two-pane Console editor, the
admin-api↔manager relay, template versioning (`version` +
`migrate_instructions`), the per-user version stamp, and the login-time
LLM-driven migration.

**Out of scope:** retroactively pushing a template to users who are not
logging in (no background sweep); a per-version migration *chain* (we apply a
single "reconcile to current" instruction once); multi-manager-per-app-code
routing (one manager — the admin-api default chat app — owns every template).

## Decisions (settled in brainstorming)

1. **Editor placement:** on the **nested OIDC-client row** of the Application
   detail page. The template binds to a specific login client
   (`app_code ↔ clientId`). admin-api resolves `clientId → app_code` from
   `secrets/app_codes.json` **server-side** (the client never maps ids).
2. **Editor UX:** two-pane **tree + content editor**, serialized to the flat
   `TemplateEntry[]`.
3. **Versioning:** monotonic **integer**. The version increases **only** when
   an operator explicitly **Publishes** a new version (a plain Save edits the
   current version's content). Publishing **requires** migration instructions.
4. **Per-user stamp:** a **marker file in the box** —
   `{userId}/{app}/.llm-chat/version` — worker-owned. Missing / unreadable /
   malformed → treated as **v0** (full provision), fail-closed.
5. **Migration model:** a **single "reconcile to current"** instruction,
   applied once even if the user is several versions behind.
6. **Migration timing:** **async / best-effort** — login returns immediately;
   the worker migrates in the background; the stamp is bumped only on success;
   on failure it is left unchanged and retried next login.
7. **Fail-closed** throughout (see §8).

## Architecture

```
admin-web  — two-pane editor on the OIDC-client row
   │  GET/PUT /api/projects/{pid}/apps/{appId}/sandbox-template
   ▼
admin-api (stateless) — clientId → app_code via secrets/app_codes.json;
   │                      mint SA token; relay to manager /control
   │  control_request {cmd:"get-template"|"set-template", ...}
   ▼
manager — app_sandbox_template gains version + migrate_instructions;
   │        handle_provision passes them through
   │  /provision {app} → call_backend {cmd:"provision-app-box",
   │                                    version, files, migrateInstructions}
   ▼
worker  — owns the sandbox; reads/writes the version stamp; runs the LLM
           migration confined to {userId}/{app}/, in a background task
```

The gateway login path is unchanged in shape: kabytech still calls
`/provision {app}`; the userId still comes from the verified token.

## Component 1 — Data model (manager `chat_db`)

Extend `app_sandbox_template` (today: `app_code PK, template_json,
updated_at`):

| column | type | meaning |
|---|---|---|
| `version` | `INTEGER NOT NULL DEFAULT 1` | monotonic; bumped only on Publish |
| `migrate_instructions` | `TEXT` (nullable) | prose to reconcile an older box **to this version** |

- Add the columns in both `init_schema_sqlite` and `init_schema_postgres`,
  **and** via an idempotent `ALTER TABLE … ADD COLUMN` guard so existing
  databases pick them up (an existing kabytech row becomes
  `version=1, migrate_instructions=NULL`).
- `KABYTECH_DEFAULT_TEMPLATE` seeds at `version=1`,
  `migrate_instructions=NULL`.

**`ChatDb` API change:**

```rust
pub struct TemplateRecord {
    pub template_json: String,
    pub version: i64,
    pub migrate_instructions: Option<String>,
    pub updated_at: String,
}

// was: get_template(app_code) -> Option<String>
pub async fn get_template(&self, app_code: &str)
    -> Result<Option<TemplateRecord>, sqlx::Error>;

// was: upsert_template(app_code, template_json, updated_at)
pub async fn upsert_template(
    &self,
    app_code: &str,
    template_json: &str,
    version: i64,
    migrate_instructions: Option<&str>,
    updated_at: &str,
) -> Result<(), sqlx::Error>;
```

All existing callers (startup seed, `handle_provision`, tests) updated to the
new shapes.

## Component 2 — Per-user version stamp (worker)

`{userId}/{app}/.llm-chat/version` — a small file holding the integer (a
trailing newline tolerated). Read/written only through the existing
`confine_path(base, user_id, "{app}/.llm-chat/version")`.

```rust
// PURE helpers (worker/src/user_env.rs)
pub fn parse_stamp(raw: Option<&str>) -> i64; // None/empty/non-int → 0
pub fn read_stamp(base: &Path, user_id: &str, app: &str) -> Result<i64, ResolveError>;
pub fn write_stamp(base: &Path, user_id: &str, app: &str, version: i64) -> Result<(), ResolveError>;
```

Fail-closed: any read error / malformed content → caller treats the box as
**v0** (do a full provision). The stamp is written **by the worker only**,
never by the migration's claude run.

## Component 3 — Provisioning + migration decision (worker)

A **pure** decision function isolates the branch logic for testing:

```rust
pub enum ProvisionAction { Provision, Current, Migrate }

/// PURE. `stamp` is the box's recorded version (0 = fresh/unprovable);
/// `target` is the template's current version.
pub fn decide_action(stamp: i64, target: i64) -> ProvisionAction {
    if stamp <= 0 { ProvisionAction::Provision }
    else if stamp >= target { ProvisionAction::Current }
    else { ProvisionAction::Migrate }
}
```

Rules:
- `stamp <= 0` → **Provision** (fresh box, or unprovable/malformed stamp).
- `stamp >= target` → **Current** (no-op).
- `0 < stamp < target` → **Migrate**.

`provision-app-box` control handler (worker), given
`{userId, app, version, files, migrateInstructions}`:

1. `resolve_user_cwd(base, userId, Some(app))` → confined app dir (refuse on
   error).
2. `read_stamp` → `stamp`; `decide_action(stamp, version)`.
3. **Provision:** `provision_entries(...)` (write-if-absent, unchanged) →
   `write_stamp(version)` → reply `{ok:true, action:"provisioned", version, created}`.
4. **Current:** reply `{ok:true, action:"current", version}`.
5. **Migrate:** first `provision_entries(...)` (so any newly-added files
   exist), then **spawn a background task** (`tokio::spawn`) that runs the LLM
   migration (§Component 4). Reply **immediately**
   `{ok:true, action:"migrating", version}`. The background task writes the
   stamp **only on success**.

A per-`(userId, app)` in-flight guard (e.g. a `Mutex<HashSet<(String,String)>>`
in worker state) skips starting a second migration while one is running.

## Component 4 — LLM migration (worker, background task)

- Run claude **non-interactively** with `--output-format stream-json` (the
  source-of-truth rule — read the structured result, never scrape a TTY),
  cwd = the canonicalized `{base}/{userId}/{app}/`.
- Prompt = the operator-authored `migrateInstructions` + a rendered manifest
  of the desired template (the substituted `TemplateEntry[]` for this user) so
  the model knows the target layout.
- **Success** = claude exits cleanly and the stream-json result event reports
  no error → `write_stamp(version)`.
- **Failure** (spawn error, non-zero exit, error result, timeout) → log;
  **leave the stamp unchanged** (retried next login); never delete user data.
- Confinement: cwd is the proven-canonical app subfolder only; if it cannot be
  proven, the migration is refused before claude is spawned.

**Honest limitation (stated in the spec on purpose):** success verifies the
*process* completed, not that the model's edits are semantically perfect.
Mitigations: operator-authored, idempotent instructions; the write-if-absent
base layer; operator-gated version bumps.

## Component 5 — manager `/control` vocabulary (chat.admin-gated)

Added to `handle_control` (already chat.admin-gated at the handshake):

- `get-template {appCode}` →
  `{ok:true, appCode, version, template:[…parsed entries…], migrateInstructions, updatedAt}`.
  Missing row → `{ok:true, version:0, template:[], migrateInstructions:null}`
  (an unconfigured app, editor starts empty).
- `set-template {appCode, template, publish:bool, migrateInstructions?}`:
  - validate `template` via the existing `parse_template` (fail-closed on bad
    entries);
  - **the manager computes the version** (client cannot set it): load current;
    - **no current row** → new version `1` (the `publish` flag is irrelevant —
      there is nothing to migrate *from*; instructions, if sent, are ignored);
    - current row, `publish:false` → keep the current version;
    - current row, `publish:true` → `current+1` and **require** non-empty
      `migrateInstructions` (else `{ok:false, error}`);
  - `upsert_template(...)` with `updated_at = now`;
  - reply `{ok:true, appCode, version, updatedAt}`.

`handle_provision` (manager): load the `TemplateRecord`, substitute variables
into each entry's content as today, and pass `version` + `migrateInstructions`
(instructions are **not** variable-substituted — they are operator prose for
the model) to the worker's `provision-app-box`.

## Component 6 — admin-api

- **App-code registry:** load `secrets/app_codes.json` at startup via
  `ADMIN_APP_CODES_PATH` (a path in the existing read-only `/secrets` mount).
  Set-but-unreadable / malformed → **fail fast** at startup; absent → the
  feature is simply off (no annotations, endpoints return `configured:false`).
  Build `clientId → {appCode, name}`.
- **Annotate** `list_project_apps`: each app gains `appCode: string | null`,
  resolved from its `oidcConfig.clientId` (already present in that response —
  no extra Zitadel calls). The Console shows the editor only where non-null.
- **Endpoints** (nested under the client; `Operator`/chat.admin gated;
  capability-gated on the default chat app exactly like `usage`/`user_files`):
  - `GET /api/projects/{pid}/apps/{appId}/sandbox-template` — resolve
    `appId → oidcConfig.clientId` (via `st.zitadel.get_project_app`) →
    registry `→ appCode`; mint token; relay `get-template`; return
    `{configured, ok, version, template, migrateInstructions, updatedAt}`.
    Unknown/un-mapped client → `404`.
  - `PUT /api/projects/{pid}/apps/{appId}/sandbox-template` — body
    `{template, publish, migrateInstructions?}`; resolve `appCode`; relay
    `set-template`; return `{ok, version, updatedAt}`.

## Component 7 — admin-web (two-pane editor)

On the Application detail page, each OIDC client with a non-null `appCode`
shows a **Sandbox template** editor:

- **Layout:** file/folder tree (left), content editor for the selected file
  (right), current-version badge, variable hints
  (`{{name}} {{userId}} {{app}} {{date}}`).
- **Actions:** **Save** (content edit — `publish:false`) and **Publish new
  version** (opens a dialog that **requires** migration instructions —
  `publish:true`). After Publish, the badge shows the new version.
- **Tree ↔ entries:** pure, unit-tested helpers:

  ```ts
  function entriesToTree(entries: TemplateEntry[]): TreeNode
  function treeToEntries(tree: TreeNode): TemplateEntry[]
  ```

  Intermediate folders are implied by file paths; explicit `dir:true` entries
  are emitted only for **empty** folders. Round-trip is stable.
- **Validation:** client mirrors the server path rules (reject `..`, absolute,
  `\`, `:`, NUL) for fast feedback; the server remains authoritative
  (`parse_template` + `confine_path`).
- **API/types:** add methods to `lib/api` and `SandboxTemplate` /
  `TemplateEntry` to `lib/types`.

## Data flow (end to end)

1. Operator opens an Application → an OIDC client with an `appCode` → edits the
   tree → **Save** (or **Publish new version** + instructions).
2. admin-web `PUT …/sandbox-template` → admin-api resolves `appCode`, relays
   `set-template` → manager validates, computes the version, upserts.
3. A user logs into kabytech → `/provision {app}` → manager loads the record,
   substitutes variables, calls the worker's `provision-app-box` with
   `version` + `migrateInstructions`.
4. Worker reads the stamp and acts: **provision** (fresh), **no-op**
   (current), or **migrate** (background LLM run, stamp bumped on success).

## Error handling & security (fail-closed)

- Migration cwd is the proven-canonical `{userId}/{app}/` only; unprovable →
  refuse.
- Migration instructions originate **only** from the operator-authored store —
  never user input.
- The stamp is bumped **only** on verified claude success; failure leaves it
  unchanged and never destroys data; login is never blocked.
- The manager computes the version — the client cannot spoof it; a `publish`
  with empty instructions is rejected.
- `set-template` rejects malformed entries (`parse_template`).
- admin-api registry: set-but-unreadable path → fail fast at startup
  (no silent default).
- Cost note: each migration is one billed claude run per user on the worker
  host's shared claude auth (same model as `/chat`); operator-gated bumps keep
  the volume controlled.

## Testing

- **Rust (manager/worker):** new columns + `ALTER` idempotency; `set-template`
  version logic (publish bumps + requires instructions; save keeps);
  `parse_stamp` (None/empty/non-int → 0); **pure** `decide_action`
  (provision/current/migrate); reuse of `parse_template`/`confine_path`. The
  claude subprocess itself is not unit-tested; the decision and stamp logic
  around it are.
- **admin-api:** registry load + `clientId → appCode`; `list_project_apps`
  annotation; endpoint `Operator` gating; `appId → appCode` resolution
  (mocked Zitadel).
- **admin-web (vitest):** `entriesToTree`/`treeToEntries` round-trip +
  validation; editor render / save / publish-requires-instructions.
- **Live e2e:** author v1 → login provisions + stamps v1; publish v2 with
  instructions → re-login migrates the folder + bumps the stamp; bad
  instructions → stamp unchanged, retried.

## File structure (anticipated)

- `manager/src/main.rs` — schema columns + `ALTER`; `TemplateRecord`;
  `get_template`/`upsert_template` signatures; `get-template`/`set-template`
  in `handle_control`; `handle_provision` passes version + instructions;
  seed at v1.
- `worker/src/user_env.rs` — `parse_stamp`/`read_stamp`/`write_stamp`;
  `decide_action`/`ProvisionAction`.
- `worker/src/lib.rs` — `provision-app-box` extended (version, instructions,
  stamp, decision); background migration task + in-flight guard; confined
  claude (stream-json) invocation.
- `admin-api/src/config.rs` — app-code registry (`ADMIN_APP_CODES_PATH`).
- `admin-api/src/api/mod.rs` — `list_project_apps` annotation; two
  `sandbox-template` routes + handlers.
- `admin-web/app/(dash)/applications/[id]/page.tsx` — wire the editor into the
  client row.
- `admin-web/components/applications/sandbox-template-editor.tsx` (+ a
  tree-utils module + vitest tests); `admin-web/lib/api`, `admin-web/lib/types`.
- `.env.local.example` / `.env.local` — `ADMIN_APP_CODES_PATH`.
- docker-compose — verify `secrets/app_codes.json` is in the admin-api
  `/secrets` mount.
