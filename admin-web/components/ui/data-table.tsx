"use client";
import {
  type ColumnDef, flexRender, getCoreRowModel,
  getPaginationRowModel, getSortedRowModel, getFilteredRowModel,
  useReactTable, type SortingState, type ColumnFiltersState,
  type VisibilityState, type OnChangeFn,
} from "@tanstack/react-table";
import { useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, ArrowUpDown, Columns3, Filter, X } from "lucide-react";
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu, DropdownMenuContent,
  DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";

/** A column's stable id from its def (accessorKey or id). */
function colId<TData, TValue>(c: ColumnDef<TData, TValue>): string | undefined {
  return "accessorKey" in c && c.accessorKey != null ? String(c.accessorKey) : c.id;
}

const PAGE_SIZE_OPTIONS = [10, 20, 50, 100];

/** The funnel toggle for a DataTable's filter panel. Render it inside a page
 * header (next to the page actions) and wire it to the same `filterOpen` state
 * you pass to <DataTable filterOpen onFilterOpenChange />. */
export function TableFilterToggle({
  open, onToggle,
}: {
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <Button
      variant="outline"
      size="sm"
      aria-label={open ? "Hide filter" : "Show filter"}
      aria-expanded={open}
      className="size-8 p-0"
      onClick={onToggle}
    >
      <Filter className="size-4" />
    </Button>
  );
}

/** A "show/hide columns" dropdown for a DataTable. Render it in the page header
 * (next to the filter toggle) and wire it to the same `columnVisibility` state
 * you pass to <DataTable columnVisibility onColumnVisibilityChange />. The
 * actions column (no header) is never hideable. */
export function TableColumnsToggle<TData, TValue>({
  columns, visibility, onChange,
}: {
  columns: ColumnDef<TData, TValue>[];
  visibility: VisibilityState;
  onChange: (next: VisibilityState) => void;
}) {
  const hideable = columns
    .map((c) => ({ id: colId(c), label: typeof c.header === "string" ? c.header : colId(c) }))
    .filter((c): c is { id: string; label: string } => !!c.id && c.id !== "actions");
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="size-8 p-0" aria-label="Show or hide columns">
          <Columns3 className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        <DropdownMenuLabel>Columns</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <div className="space-y-0.5 p-1">
          {hideable.map((c) => {
            const on = visibility[c.id] !== false;
            return (
              <div
                key={c.id}
                role="menuitemcheckbox"
                aria-checked={on}
                onClick={() => onChange({ ...visibility, [c.id]: !on })}
                className="hover:bg-accent flex cursor-pointer items-center justify-between gap-3 rounded-sm px-2 py-1.5 text-sm"
              >
                <span>{c.label}</span>
                <Switch checked={on} className="pointer-events-none" />
              </div>
            );
          })}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

interface DataTableProps<TData, TValue> {
  columns: ColumnDef<TData, TValue>[];
  data: TData[];
  filterColumn?: string;
  filterPlaceholder?: string;
  emptyMessage?: string;
  /** Optional extra controls rendered to the right of the filter input. */
  toolbar?: ReactNode;
  /** Initial rows per page (user-adjustable via the footer selector). */
  pageSize?: number;
  /** Stable id for a row — enables selection highlighting + row-click detail. */
  getRowId?: (row: TData) => string;
  /** Called when a row body is clicked (ignored on interactive cells like the
   * actions menu). Wire this to open a side detail panel. */
  onRowClick?: (row: TData) => void;
  /** Currently-selected row id (highlights that row). */
  selectedRowId?: string | null;
  /** Controlled filter-panel open state — pass this (with onFilterOpenChange)
   * to render the filter toggle up in the page header instead of the table. */
  filterOpen?: boolean;
  onFilterOpenChange?: (open: boolean) => void;
  /** Controlled column visibility — pass this (with onColumnVisibilityChange)
   * to drive a TableColumnsToggle rendered in the page header. */
  columnVisibility?: VisibilityState;
  onColumnVisibilityChange?: OnChangeFn<VisibilityState>;
}

export function DataTable<TData, TValue>({
  columns, data, filterColumn, filterPlaceholder, emptyMessage = "No results.",
  toolbar, pageSize: initialPageSize = 10,
  getRowId, onRowClick, selectedRowId,
  filterOpen: filterOpenProp, onFilterOpenChange,
  columnVisibility, onColumnVisibilityChange,
}: DataTableProps<TData, TValue>) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [columnFilters, setColumnFilters] = useState<ColumnFiltersState>([]);
  const [internalVisibility, setInternalVisibility] = useState<VisibilityState>({});
  const [internalFilterOpen, setInternalFilterOpen] = useState(false);
  // Controlled when the page lifts the state (to render the toggle in the page
  // header); otherwise DataTable owns it and renders its own toggle.
  const filterControlled = onFilterOpenChange !== undefined;
  const filterOpen = filterControlled ? !!filterOpenProp : internalFilterOpen;
  const setFilterOpen = (open: boolean) =>
    filterControlled ? onFilterOpenChange(open) : setInternalFilterOpen(open);
  const table = useReactTable({
    data, columns,
    getCoreRowModel: getCoreRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    onSortingChange: setSorting,
    onColumnFiltersChange: setColumnFilters,
    onColumnVisibilityChange: onColumnVisibilityChange ?? setInternalVisibility,
    state: {
      sorting,
      columnFilters,
      columnVisibility: columnVisibility ?? internalVisibility,
    },
    initialState: { pagination: { pageSize: initialPageSize } },
  });

  const { pageIndex, pageSize } = table.getState().pagination;
  const total = table.getFilteredRowModel().rows.length;
  const rangeStart = total === 0 ? 0 : pageIndex * pageSize + 1;
  const rangeEnd = Math.min(total, (pageIndex + 1) * pageSize);

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {/* When the filter state is CONTROLLED the page renders the toggle up in
          its header, so DataTable shows a toolbar row only for `toolbar` or its
          own (uncontrolled) funnel. The funnel opens a FILTER PANEL that expands
          from the left of the table (mirroring the right detail panel). */}
      {(toolbar || (filterColumn && !filterControlled)) && (
        <div className="flex shrink-0 items-center justify-end gap-2">
          {toolbar}
          {filterColumn && !filterControlled && (
            <TableFilterToggle open={filterOpen} onToggle={() => setFilterOpen(!filterOpen)} />
          )}
        </div>
      )}
      {/* Body: an optional left FILTER PANEL (expands from the left, mirroring
          the right-hand detail panel) sitting beside the table. */}
      <div className="flex min-h-0 flex-1 gap-3">
        {filterColumn && filterOpen && (
          <aside className="bg-card animate-in slide-in-from-left-2 fade-in-0 flex w-60 shrink-0 flex-col rounded-xl border p-3 shadow-sm duration-200">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-muted-foreground text-[11px] font-semibold tracking-wide uppercase">
                Filter
              </span>
              <button
                type="button"
                aria-label="Close filter"
                className="text-muted-foreground hover:bg-muted hover:text-foreground rounded-md p-1 transition-colors"
                onClick={() => {
                  table.getColumn(filterColumn)?.setFilterValue("");
                  setFilterOpen(false);
                }}
              >
                <X className="size-4" />
              </button>
            </div>
            <Input
              autoFocus
              placeholder={filterPlaceholder ?? "Filter..."}
              value={(table.getColumn(filterColumn)?.getFilterValue() as string) ?? ""}
              onChange={(e) =>
                table.getColumn(filterColumn)?.setFilterValue(e.target.value)
              }
            />
          </aside>
        )}
        {/* This div is the ONLY scroll container so the sticky thead pins to it
            (the shadcn <Table> wrapper adds its own overflow-x box, which would
            capture position:sticky — hence the raw <table> here). */}
        <div className="min-h-0 flex-1 overflow-auto rounded-xl border bg-card shadow-sm">
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
              table.getRowModel().rows.map((row) => {
                const rowId = getRowId?.(row.original);
                const isSelected =
                  rowId != null && selectedRowId != null && rowId === selectedRowId;
                return (
                  <TableRow
                    key={row.id}
                    data-selected={isSelected || undefined}
                    aria-selected={onRowClick ? isSelected : undefined}
                    className={
                      "hover:bg-muted/50 data-[selected]:bg-primary/5 " +
                      "data-[selected]:shadow-[inset_2px_0_0_var(--primary)] " +
                      (onRowClick ? "cursor-pointer" : "")
                    }
                    onClick={
                      onRowClick
                        ? (e) => {
                            // Don't hijack clicks on interactive cells (actions
                            // menu, links, avatar-stack buttons, inputs).
                            if (
                              (e.target as HTMLElement).closest(
                                "button, a, input, select, [role='menuitem']",
                              )
                            )
                              return;
                            onRowClick(row.original);
                          }
                        : undefined
                    }
                  >
                    {row.getVisibleCells().map((cell) => (
                      <TableCell key={cell.id} className="px-3 py-2.5">
                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                      </TableCell>
                    ))}
                  </TableRow>
                );
              })
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
      </div>
      <div className="flex shrink-0 items-center justify-between gap-2">
        <span className="text-muted-foreground text-sm tabular-nums">
          {rangeStart}–{rangeEnd} of {total}
        </span>
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2">
            <span className="text-muted-foreground text-sm">Rows per page</span>
            <Select
              value={String(pageSize)}
              onValueChange={(v) => table.setPageSize(Number(v))}
            >
              <SelectTrigger
                aria-label="Rows per page"
                className="h-8 w-[4.75rem] text-sm"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PAGE_SIZE_OPTIONS.map((n) => (
                  <SelectItem key={n} value={String(n)}>{n}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
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
    </div>
  );
}
