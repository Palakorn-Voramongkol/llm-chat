import { test, expect } from "@playwright/test";
import { login } from "./auth";

const FULL = process.env.ADMIN_IT === "1";

test.describe("console shell", () => {
  test.skip(!FULL, "requires running stack: set ADMIN_IT=1 + a chat.admin session");

  test.beforeEach(async ({ page }) => {
    await login(page);
  });

  test("renders the activity-bar nav and highlights the active area", async ({ page }) => {
    // login() lands on /dashboard; navigate to /users so usePathname marks the
    // Users area active (aria-current=page) for the highlight assertion below.
    await page.goto("/users");
    await expect(page.getByRole("heading", { name: "Users" })).toBeVisible();

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
