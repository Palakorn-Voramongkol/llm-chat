import { describe, it, expect } from "vitest";
import type { ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import type { ColumnDef } from "@tanstack/react-table";
import type { AppProject } from "@/lib/types";
import { buildApplicationColumns, type AppMeta } from "./columns";

const colId = (c: ColumnDef<AppProject>): string | undefined =>
  // tanstack columns carry either an explicit `id` or an `accessorKey`.
  (c as { id?: string }).id ?? (c as { accessorKey?: string }).accessorKey;

describe("application columns", () => {
  it("includes a clients column", () => {
    const ids = buildApplicationColumns(new Map()).map(colId);
    expect(ids).toContain("clients");
  });

  it("renders the client count from AppMeta", () => {
    const meta: AppMeta = { roleKeys: [], userCount: 0, clientCount: 3 };
    const clients = buildApplicationColumns(new Map([["p1", meta]])).find(
      (c) => colId(c) === "clients",
    )!;
    const renderCell = clients.cell as (ctx: { row: { original: AppProject } }) => ReactNode;
    render(<>{renderCell({ row: { original: { id: "p1" } } })}</>);
    expect(screen.getByText("3")).toBeInTheDocument();
  });
});
