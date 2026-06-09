import { test, expect } from "@playwright/test";

const FULL = process.env.ADMIN_IT === "1";

test.describe("console shell", () => {
  test.skip(!FULL, "requires running stack: set ADMIN_IT=1 + a chat.admin session");

  test("renders the activity-bar nav and highlights the active area", async ({ page }) => {
    // Real login against Zitadel v3 (operator with chat.admin), same as smoke.spec.ts.
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

    await page.waitForURL(/\/users/);

    // The activity-bar ribbon renders every NAV area.
    const nav = page.getByRole("navigation", { name: "Primary" });
    for (const label of ["Dashboard", "Users", "Roles", "Applications", "Project & Org", "Audit"]) {
      await expect(nav.getByRole("link", { name: new RegExp(label, "i") })).toBeVisible();
    }

    // usePathname marks Users active; the others are not current.
    await expect(nav.getByRole("link", { name: /Users/i })).toHaveAttribute("aria-current", "page");
    await expect(nav.getByRole("link", { name: /Roles/i })).not.toHaveAttribute("aria-current", "page");

    // Topbar breadcrumb + operator badge are present (shell, not page).
    await expect(page.getByText("Console /")).toBeVisible();
  });
});
