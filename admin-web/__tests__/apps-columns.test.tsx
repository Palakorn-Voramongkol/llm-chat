import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { flexRender } from "@tanstack/react-table";
import { buildAppColumns } from "../components/apps/columns";
import type { OidcApp } from "../lib/types";

const app: OidcApp = {
  id: "a1", name: "Chat",
  oidcConfig: { clientId: "c1", appType: "OIDC_APP_TYPE_WEB", authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC" },
};

describe("app columns", () => {
  it("has name, clientId, appType, actions columns", () => {
    const ids = buildAppColumns({ onEdit: vi.fn(), onRotate: vi.fn(), onDelete: vi.fn() })
      .map((c) => ("accessorKey" in c ? (c as any).accessorKey : c.id));
    expect(ids).toEqual(expect.arrayContaining(["name", "clientId", "appType", "actions"]));
  });

  it("fires onRotate from the row action menu", async () => {
    const onRotate = vi.fn();
    const cols = buildAppColumns({ onEdit: vi.fn(), onRotate, onDelete: vi.fn() });
    const actions = cols.find((c) => c.id === "actions")!;
    const ctx = { row: { original: app } };
    render(<>{flexRender((actions as any).cell, ctx as any)}</>);
    const trigger = screen.getByRole("button");
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.click(trigger);
    const item = await screen.findByTestId("action-rotate-secret");
    item.click();
    expect(onRotate).toHaveBeenCalledWith(app);
  });
});
