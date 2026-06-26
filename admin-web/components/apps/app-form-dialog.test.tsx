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
