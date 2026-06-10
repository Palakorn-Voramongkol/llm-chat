"use client";
import {
  type ColumnDef, flexRender, getCoreRowModel,
  getPaginationRowModel, getSortedRowModel, getFilteredRowModel,
  useReactTable, type SortingState, type ColumnFiltersState,
} from "@tanstack/react-table";
import { useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";

interface DataTableProps<TData, TValue> {
  columns: ColumnDef<TData, TValue>[];
  data: TData[];
  filterColumn?: string;
  filterPlaceholder?: string;
  emptyMessage?: string;
  /** Optional extra controls rendered to the right of the filter input. */
  toolbar?: ReactNode;
}

export function DataTable<TData, TValue>({
  columns, data, filterColumn, filterPlaceholder, emptyMessage = "No results.",
  toolbar,
}: DataTableProps<TData, TValue>) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [columnFilters, setColumnFilters] = useState<ColumnFiltersState>([]);
  const table = useReactTable({
    data, columns,
    getCoreRowModel: getCoreRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    onSortingChange: setSorting,
    onColumnFiltersChange: setColumnFilters,
    state: { sorting, columnFilters },
    initialState: { pagination: { pageSize: 10 } },
  });

  const { pageIndex, pageSize } = table.getState().pagination;
  const total = table.getFilteredRowModel().rows.length;
  const rangeStart = total === 0 ? 0 : pageIndex * pageSize + 1;
  const rangeEnd = Math.min(total, (pageIndex + 1) * pageSize);

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {(filterColumn || toolbar) && (
        <div className="flex shrink-0 items-center gap-2">
          {filterColumn && (
            <Input
              placeholder={filterPlaceholder ?? "Filter..."}
              value={(table.getColumn(filterColumn)?.getFilterValue() as string) ?? ""}
              onChange={(e) =>
                table.getColumn(filterColumn)?.setFilterValue(e.target.value)
              }
              className="max-w-sm"
            />
          )}
          {toolbar}
        </div>
      )}
      {/* This div is the ONLY scroll container so the sticky thead pins to it
          (the shadcn <Table> wrapper adds its own overflow-x box, which would
          capture position:sticky — hence the raw <table> here). */}
      <div className="flex-1 min-h-0 overflow-auto rounded-xl border bg-card shadow-sm">
        <table data-slot="table" className="w-full caption-bottom text-sm">
          <TableHeader>
            {table.getHeaderGroups().map((hg) => (
              <TableRow key={hg.id} className="hover:bg-transparent">
                {hg.headers.map((h) => {
                  const canSort = h.column.getCanSort();
                  const dir = h.column.getIsSorted();
                  return (
                    <TableHead
                      key={h.id}
                      className="sticky top-0 z-10 bg-card px-3 text-xs font-semibold uppercase tracking-wide text-muted-foreground shadow-[inset_0_-1px_0_var(--border)]"
                    >
                      {h.isPlaceholder ? null : canSort ? (
                        <button
                          type="button"
                          className="inline-flex items-center gap-1 uppercase tracking-wide hover:text-foreground transition-colors"
                          onClick={h.column.getToggleSortingHandler()}
                        >
                          {flexRender(h.column.columnDef.header, h.getContext())}
                          {dir === "asc" ? (
                            <ArrowUp className="size-3.5" />
                          ) : dir === "desc" ? (
                            <ArrowDown className="size-3.5" />
                          ) : (
                            <ArrowUpDown className="size-3.5 opacity-50" />
                          )}
                        </button>
                      ) : (
                        flexRender(h.column.columnDef.header, h.getContext())
                      )}
                    </TableHead>
                  );
                })}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id} className="hover:bg-muted/50">
                  {row.getVisibleCells().map((cell) => (
                    <TableCell key={cell.id} className="px-3 py-2.5">
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={columns.length}
                  className="h-24 text-center text-muted-foreground">
                  {emptyMessage}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </table>
      </div>
      <div className="flex shrink-0 items-center justify-between gap-2">
        <span className="text-muted-foreground text-sm tabular-nums">
          {rangeStart}–{rangeEnd} of {total}
        </span>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm"
            onClick={() => table.previousPage()}
            disabled={!table.getCanPreviousPage()}>Previous</Button>
          <Button variant="outline" size="sm"
            onClick={() => table.nextPage()}
            disabled={!table.getCanNextPage()}>Next</Button>
        </div>
      </div>
    </div>
  );
}
