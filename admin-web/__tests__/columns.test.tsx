import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { flexRender } from "@tanstack/react-table";
import { buildColumns } from "../components/users/columns";
import type { User } from "../lib/types";

const human: User = {
  id: "u1", userName: "alice", kind: "Human", state: "ACTIVE",
  email: "alice@x.io", displayName: "Alice A",
};

function renderCell(colId: string, user: User) {
  const cols = buildColumns({
    onEdit: vi.fn(), onDelete: vi.fn(), onLifecycle: vi.fn(), onGrants: vi.fn(),
  });
  const col = cols.find((c) => ("accessorKey" in c ? c.accessorKey : c.id) === colId);
  if (!col) throw new Error(`no column ${colId}`);
  const cell = (col as any).cell;
  // minimal row stub for cell renderers
  const ctx = { row: { original: user, getValue: (k: string) => (user as any)[k] } };
  return render(<>{flexRender(cell, ctx as any)}</>);
}

describe("user columns", () => {
  it("has the expected columns", () => {
    const ids = buildColumns({ onEdit: vi.fn(), onDelete: vi.fn(), onLifecycle: vi.fn(), onGrants: vi.fn() })
      .map((c) => ("accessorKey" in c ? (c as any).accessorKey : c.id));
    expect(ids).toEqual(
      expect.arrayContaining(["userName", "kind", "roles", "state", "actions"]),
    );
  });

  it("renders state as a friendly title-cased label", () => {
    renderCell("state", human);
    expect(screen.getByText("Active")).toBeInTheDocument();
  });

  it("fires onLifecycle('deactivate') from the row action menu", async () => {
    const onLifecycle = vi.fn();
    const cols = buildColumns({ onEdit: vi.fn(), onDelete: vi.fn(), onLifecycle, onGrants: vi.fn() });
    const actions = cols.find((c) => c.id === "actions")!;
    const ctx = { row: { original: human } };
    render(<>{flexRender((actions as any).cell, ctx as any)}</>);
    // Radix DropdownMenu renders items into a portal only when open;
    // Radix listens to pointerdown+click on the trigger to open.
    const trigger = screen.getByRole("button");
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.click(trigger);
    // the menu items are rendered with data-testid attributes for deterministic testing
    const item = await screen.findByTestId("action-deactivate");
    item.click();
    expect(onLifecycle).toHaveBeenCalledWith(human, "deactivate");
  });
});
