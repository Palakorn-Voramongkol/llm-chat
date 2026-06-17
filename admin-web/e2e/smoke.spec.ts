import { test, expect, type Page } from "@playwright/test";
import { login } from "./auth";

const FULL = process.env.ADMIN_IT === "1";

// The table filter is collapsed behind a "Show filter" toggle and uses
// "Search <field>…" placeholders. Open it if not already shown, then fill.
async function filterBy(page: Page, placeholder: RegExp, value: string) {
  const input = page.getByPlaceholder(placeholder);
  if (!(await input.isVisible().catch(() => false))) {
    await page.getByRole("button", { name: "Show filter" }).click();
  }
  await input.fill(value);
}

// The shell's OperatorBadge fetches /api/status; mocked (no-session) tests must
// stub it or lib/api 401-redirects the page to /login before content renders.
function mockOperatorStatus(page: Page, events: boolean) {
  return page.route("**/api/status", (r) =>
    r.fulfill({
      json: {
        operator: { userId: "op-1", name: "operator", roles: ["chat.admin"] },
        session: { expiresAt: null },
        health: { zitadel: true },
        capabilities: { events, chatSessions: false },
      },
    }),
  );
}

test("unauthenticated visit to /users redirects toward /login (BFF nav)", async ({ page }) => {
  // No session cookie: lib/api 401 -> window.location.assign('/login'),
  // which the same-origin proxy forwards to admin-api -> Zitadel /authorize.
  const resp = await page.goto("/users");
  // Either the client redirected us to /login, or (full stack) on to Zitadel.
  // The redirect is client-side (hydrate -> fetch /api/users -> 401 ->
  // location.assign('/login')); allow 15s so a slow hydration under full-suite
  // load doesn't flake (passes ~1.2s in isolation).
  await expect
    .poll(() => page.url(), { timeout: 15000 })
    .toMatch(/\/login|\/oauth\/v2\/authorize/);
  expect(resp).not.toBeNull();
});

test("audit page fails closed: capabilities.events=false shows the IAM_OWNER_VIEWER banner", async ({ page }) => {
  // Force the capability probe to report "no events" — the page must not error,
  // it must show the fail-closed banner (design §11).
  await page.route("**/api/me", (r) =>
    r.fulfill({ json: { userId: "op-1", name: "operator", roles: ["chat.admin"] } }),
  );
  await mockOperatorStatus(page, false);
  await page.route("**/api/capabilities", (r) => r.fulfill({ json: { events: false } }));
  // If the page ever calls /api/events with the capability off, fail loudly.
  let eventsCalled = false;
  await page.route("**/api/events*", (r) => {
    eventsCalled = true;
    return r.fulfill({ json: { result: [] } });
  });

  await page.goto("/audit");
  await expect(
    page.getByText("Audit requires IAM_OWNER_VIEWER on the service account"),
  ).toBeVisible();
  expect(eventsCalled, "must not fetch events when capability is false").toBe(false);
});

test("audit page with capability on lists events", async ({ page }) => {
  // Capability ON: the page must fetch /api/events and render the row through
  // auditColumns into the DataTable (the non-banner branch, design §11).
  await page.route("**/api/me", (r) =>
    r.fulfill({ json: { userId: "op-1", name: "operator", roles: ["chat.admin"] } }),
  );
  await mockOperatorStatus(page, true);
  await page.route("**/api/capabilities", (r) => r.fulfill({ json: { events: true } }));
  await page.route("**/api/events*", (r) =>
    r.fulfill({
      json: {
        result: [
          {
            sequence: "42",
            creationDate: "2026-06-01T10:00:00Z",
            type: { type: "user.human.added", localized: { localizedMessage: "User added" } },
            editor: { userId: "u-9", displayName: "Operator One" },
            aggregate: { id: "u-9", type: "user" },
          },
        ],
      },
    }),
  );

  await page.goto("/audit");
  await expect(page.getByText("User added")).toBeVisible();
  await expect(page.getByText("Operator One")).toBeVisible();
  // The banner must NOT be present on the capability-on path.
  await expect(
    page.getByText("Audit requires IAM_OWNER_VIEWER on the service account"),
  ).toHaveCount(0);
});

test.describe("authenticated operator flow", () => {
  test.skip(!FULL, "requires running stack: set ADMIN_IT=1 + a logged-in chat.admin session");

  // Playwright gives each test a fresh context (no shared cookies), so every
  // test in this block logs in first. The helper lands on /dashboard; each test
  // then navigates to the page it actually exercises.
  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test("login -> list users -> create machine user", async ({ page }) => {
    // The default landing is /dashboard; this flow works on the Users page.
    await page.goto("/users");
    await expect(page.getByRole("heading", { name: "Users" })).toBeVisible();

    // Create a machine user.
    const uname = `pw-bot-${Date.now()}`;
    await page.getByTestId("create-user").click();
    await page.getByRole("combobox").click();
    await page.getByRole("option", { name: "Machine" }).click();
    await page.getByLabel("Username").fill(uname);
    await page.getByLabel("Display name").fill(uname);
    await page.getByRole("button", { name: "Create" }).click();

    // New row appears (filter then assert).
    await filterBy(page, /search username/i, uname);
    await expect(page.getByText(uname)).toBeVisible();
  });

  test("dashboard is the landing and renders stat cards", async ({ page }) => {
    // The Console lands on /dashboard (design §10); / redirects there.
    await page.goto("/");
    await page.waitForURL(/\/dashboard/);
    await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();

    // Cards render against the live /api/stats fan-out: labels are always present,
    // and each count is either a number or an em-dash (never blank). Each card is
    // a link with an aria-label, so target the link (the "Apps" text also appears
    // in the page description, hence the role-scoped locator).
    await expect(page.getByRole("link", { name: "Humans" })).toBeVisible();
    await expect(page.getByRole("link", { name: "Apps" })).toBeVisible();
    const humansLink = page.getByRole("link", { name: "Humans" });
    await expect(humansLink).toHaveAttribute("href", "/users");
  });

  test("create then delete a role (cascade confirm)", async ({ page }) => {
    await page.goto("/roles");
    await expect(page.getByRole("heading", { name: "Roles" })).toBeVisible();
    const key = `pw.role.${Date.now()}`;
    await page.getByTestId("create-role").click();
    await page.getByLabel("Role key").fill(key);
    await page.getByLabel("Display name").fill("PW Role");
    await page.getByRole("button", { name: "Create" }).click();

    await filterBy(page, /search key/i, key);
    await expect(page.getByText(key)).toBeVisible();

    // Delete via the row action -> cascade confirm.
    await page.getByRole("row", { name: new RegExp(key) })
      .getByRole("button", { name: /open menu/i }).click();
    await page.getByTestId("role-delete").click();
    await page.getByRole("button", { name: "Delete role" }).click();
    await expect(page.getByText(key)).toHaveCount(0);
  });

  test("grant assign then revoke round-trip", async ({ page }) => {
    // Create a throwaway machine user, then toggle a grant on/off via the
    // one-grant-per-project branch (POST create -> DELETE revoke-all).
    await page.goto("/users");
    const uname = `pw-grant-${Date.now()}`;
    await page.getByTestId("create-user").click();
    await page.getByRole("combobox").click();
    await page.getByRole("option", { name: "Machine" }).click();
    await page.getByLabel("Username").fill(uname);
    await page.getByLabel("Display name").fill(uname);
    await page.getByRole("button", { name: "Create" }).click();
    await filterBy(page, /search username/i, uname);
    await expect(page.getByText(uname)).toBeVisible();

    // Open Access (grants), assign chat.user (POST create grant).
    await page.getByRole("row", { name: new RegExp(uname) })
      .getByRole("button", { name: /open menu/i }).click();
    await page.getByTestId("action-grants").click();
    await page.getByTestId("grant-role-chat.user").click();
    await page.getByTestId("grants-save").click();
    await expect(page.getByText("Access updated")).toBeVisible();

    // Re-open, unselect everything, save (DELETE revoke-all). The dialog
    // reloads the now-checked chat.user; unchecking + save deletes the grant.
    await page.getByRole("row", { name: new RegExp(uname) })
      .getByRole("button", { name: /open menu/i }).click();
    await page.getByTestId("action-grants").click();
    await page.getByTestId("grant-role-chat.user").click(); // uncheck
    await page.getByTestId("grants-save").click();
    await expect(page.getByText("Access updated")).toBeVisible();
  });

  test("create OIDC app reveals the client secret exactly once", async ({ page }) => {
    // Canonical route is /apps (NAV.href in components/shell/nav.ts); the page
    // heading is "Applications".
    await page.goto("/apps");
    await expect(page.getByRole("heading", { name: "Applications" })).toBeVisible();

    const appName = `pw-app-${Date.now()}`;
    await page.getByTestId("create-app").click();
    await page.getByLabel("Name").fill(appName);
    await page.getByLabel(/redirect uris/i).fill("https://example.localhost/callback");
    // appType defaults to Web (confidential) + Basic -> server returns a secret.
    await page.getByRole("button", { name: "Create" }).click();

    // the secret is revealed once, with a copy affordance.
    const secret = page.getByTestId("reveal-client-secret");
    await expect(secret).toBeVisible();
    const secretValue = await secret.inputValue();
    expect(secretValue.length).toBeGreaterThan(0);
    await expect(
      page.getByText(/shown once and cannot be retrieved again/i),
    ).toBeVisible();

    // dismiss -> the secret is gone and NOT recoverable from the list page.
    await page.getByTestId("reveal-done").click();
    await expect(page.getByTestId("reveal-client-secret")).toHaveCount(0);
    await filterBy(page, /search name/i, appName);
    await expect(page.getByText(appName)).toBeVisible();
    // the row shows clientId but never the secret value.
    await expect(page.getByText(secretValue)).toHaveCount(0);
  });

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
});
