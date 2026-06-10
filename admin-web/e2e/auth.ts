import type { Page } from "@playwright/test";
import { expect } from "@playwright/test";

/**
 * Drive the live Zitadel v3 hosted-UI login as the chat.admin operator.
 *
 * Classic flow: login name -> Next, then password -> Next. Field names come
 * from the live /ui/login form (loginName, password); buttons read "Next".
 * After the BFF sets its session cookie it 302s back to admin-web, which lands
 * on /dashboard (the default landing, design §10).
 *
 * Playwright gives each test a FRESH context (no shared cookies), so every test
 * that needs an authenticated session must call this — typically from a
 * `test.beforeEach` inside the authenticated describe block.
 */
export async function login(page: Page): Promise<void> {
  await page.goto("/login");
  await page.locator('input[name="loginName"]').fill(process.env.ADMIN_IT_USER!);
  await page.getByRole("button", { name: /next|continue/i }).click();
  await page.locator('input[name="password"]').fill(process.env.ADMIN_IT_PASS!);
  await page.getByRole("button", { name: /next|continue|sign in/i }).click();

  // New users are nudged to set up 2FA. Whether the Skip page appears depends on
  // the MFA-init skip lifetime, so click Skip if it shows within a short window
  // but tolerate its absence (login went straight through).
  const skip2fa = page.getByRole("button", { name: /skip/i });
  await skip2fa
    .waitFor({ state: "visible", timeout: 8000 })
    .then(() => skip2fa.click())
    .catch(() => {});

  // Land back on an authenticated Console page. The default landing is
  // /dashboard; assert we're off the Zitadel hosted UI and on the dashboard
  // shell before each test navigates to the page it actually exercises.
  await page.waitForURL(/\/dashboard/);
  await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
}
