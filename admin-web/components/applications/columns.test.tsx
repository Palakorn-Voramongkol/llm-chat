import { describe, it, expect } from "vitest";
import type { ColumnDef } from "@tanstack/react-table";
import type { AppProject } from "@/lib/types";
import { buildApplicationColumns } from "./columns";

const colId = (c: ColumnDef<AppProject>): string | undefined =>
  // tanstack columns carry either an explicit `id` or an `accessorKey`.
  (c as { id?: string }).id ?? (c as { accessorKey?: string }).accessorKey;

describe("application columns", () => {
  it("includes a clients column", () => {
    const ids = buildApplicationColumns(new Map()).map(colId);
    expect(ids).toContain("clients");
  });
});
