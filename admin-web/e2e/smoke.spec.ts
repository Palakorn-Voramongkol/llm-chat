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
    // Real login against Zitadel (operator with chat.admin). Credentials from env.
    await page.goto("/login");
    await page.getByLabel(/username|loginname/i).fill(process.env.ADMIN_IT_USER!);
    await page.getByRole("button", { name: /next|continue/i }).click();
    await page.getByLabel(/password/i).fill(process.env.ADMIN_IT_PASS!);
    await page.getByRole("button", { name: /next|continue|sign in/i }).click();

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
