import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import type { ColumnDef } from "@tanstack/react-table";
import { DataTable } from "../components/ui/data-table";

type Row = { name: string };
const columns: ColumnDef<Row>[] = [{ accessorKey: "name", header: "Name" }];

describe("DataTable empty state", () => {
  it("shows a neutral default message when there are no rows", () => {
    render(<DataTable columns={columns} data={[]} />);
    expect(screen.getByText("No results.")).toBeInTheDocument();
  });

  it("shows a caller-supplied emptyMessage when there are no rows", () => {
    render(<DataTable columns={columns} data={[]} emptyMessage="No roles yet." />);
    expect(screen.getByText("No roles yet.")).toBeInTheDocument();
    expect(screen.queryByText("No users.")).not.toBeInTheDocument();
  });
});
