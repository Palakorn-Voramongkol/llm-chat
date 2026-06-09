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
});
