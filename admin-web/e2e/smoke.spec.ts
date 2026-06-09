import { test, expect } from "@playwright/test";

const FULL = process.env.ADMIN_IT === "1";

test("unauthenticated visit to /users redirects toward /login (BFF nav)", async ({ page }) => {
  // No session cookie: lib/api 401 -> window.location.assign('/login'),
  // which the same-origin proxy forwards to admin-api -> Zitadel /authorize.
  const resp = await page.goto("/users");
  // Either the client redirected us to /login, or (full stack) on to Zitadel.
  await expect
    .poll(() => page.url())
    .toMatch(/\/login|\/oauth\/v2\/authorize/);
  expect(resp).not.toBeNull();
});

test.describe("authenticated operator flow", () => {
  test.skip(!FULL, "requires running stack: set ADMIN_IT=1 + a logged-in chat.admin session");

  test("login -> list users -> create machine user", async ({ page }) => {
    // Real login against the Zitadel v3 hosted UI (operator with chat.admin).
    // Classic flow: login name -> Next, then password -> Next. Field names come
    // from the live /ui/login form (loginName, password); buttons read "Next".
    await page.goto("/login");
    await page.locator('input[name="loginName"]').fill(process.env.ADMIN_IT_USER!);
    await page.getByRole("button", { name: /next|continue/i }).click();
    await page.locator('input[name="password"]').fill(process.env.ADMIN_IT_PASS!);
    await page.getByRole("button", { name: /next|continue|sign in/i }).click();

    // New users are nudged to set up 2FA; the operator skips it (optional on the
    // local stack). If login went straight through, this button never appears.
    const skip2fa = page.getByRole("button", { name: /skip/i });
    await skip2fa
      .waitFor({ state: "visible", timeout: 8000 })
      .then(() => skip2fa.click())
      .catch(() => {});

    // Lands back on the dashboard (BFF set its session cookie, 302 -> admin-web).
    await page.waitForURL(/\/users/);
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
    await page.getByPlaceholder(/filter by username/i).fill(uname);
    await expect(page.getByText(uname)).toBeVisible();
  });

  test("create then delete a role (cascade confirm)", async ({ page }) => {
    await page.goto("/roles");
    await expect(page.getByRole("heading", { name: "Roles" })).toBeVisible();
    const key = `pw.role.${Date.now()}`;
    await page.getByTestId("create-role").click();
    await page.getByLabel("Role key").fill(key);
    await page.getByLabel("Display name").fill("PW Role");
    await page.getByRole("button", { name: "Create" }).click();

    await page.getByPlaceholder(/filter by key/i).fill(key);
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
    await page.getByPlaceholder(/filter by username/i).fill(uname);
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
    await page.goto("/login");
    await page.locator('input[name="loginName"]').fill(process.env.ADMIN_IT_USER!);
    await page.getByRole("button", { name: /next|continue/i }).click();
    await page.locator('input[name="password"]').fill(process.env.ADMIN_IT_PASS!);
    await page.getByRole("button", { name: /next|continue|sign in/i }).click();
    const skip2fa = page.getByRole("button", { name: /skip/i });
    await skip2fa
      .waitFor({ state: "visible", timeout: 8000 })
      .then(() => skip2fa.click())
      .catch(() => {});

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
    await page.getByPlaceholder(/filter by name/i).fill(appName);
    await expect(page.getByText(appName)).toBeVisible();
    // the row shows clientId but never the secret value.
    await expect(page.getByText(secretValue)).toHaveCount(0);
  });
});
