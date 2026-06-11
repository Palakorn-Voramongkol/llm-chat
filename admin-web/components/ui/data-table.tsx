"use client";
import {
  type ColumnDef, flexRender, getCoreRowModel,
  getPaginationRowModel, getSortedRowModel, getFilteredRowModel,
  useReactTable, type SortingState, type ColumnFiltersState,
  type VisibilityState, type OnChangeFn,
} from "@tanstack/react-table";
import { useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, ArrowUpDown, Columns3, Filter, Rows3, X } from "lucide-react";
import { type Density, DENSITY_PADDING } from "@/lib/use-table-density";
import {
  TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import {
  DropdownMenu, DropdownMenuContent,
  DropdownMenuLabel, DropdownMenuRadioGroup, DropdownMenuRadioItem,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";

/** A column's stable id from its def (accessorKey or id). */
function colId<TData, TValue>(c: ColumnDef<TData, TValue>): string | undefined {
  return "accessorKey" in c && c.accessorKey != null ? String(c.accessorKey) : c.id;
}

/** One field in the multi-field search panel. `column` must match a DataTable
 * column id that has a real accessor. Provide `options` to render an exact-match
 * dropdown; omit it for a free-text "contains" input. */
export interface FilterField {
  column: string;
  label: string;
  placeholder?: string;
  options?: { value: string; label: string }[];
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
            const id = `colvis-${c.id}`;
            return (
              <Label
                key={c.id}
                htmlFor={id}
                className="hover:bg-accent flex cursor-pointer items-center justify-between gap-3 rounded-sm px-2 py-1.5 text-sm font-normal"
              >
                {c.label}
                <Switch
                  id={id}
                  checked={on}
                  onCheckedChange={(v) => onChange({ ...visibility, [c.id]: !!v })}
                />
              </Label>
            );
          })}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** A row-density picker (3 levels) for a DataTable. Render it in the page
 * header next to the filter/columns toggles, wired to the same density state
 * you pass to <DataTable density>. */
export function TableDensityToggle({
  density, onChange,
}: {
  density: Density;
  onChange: (d: Density) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="size-8 p-0" aria-label="Row density">
          <Rows3 className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-40">
        <DropdownMenuLabel>Row density</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup value={density} onValueChange={(v) => onChange(v as Density)}>
          <DropdownMenuRadioItem value="comfortable">Comfortable</DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="compact">Compact</DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="condensed">Condensed</DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

interface DataTableProps<TData, TValue> {
  columns: ColumnDef<TData, TValue>[];
  data: TData[];
  filterColumn?: string;
  filterPlaceholder?: string;
  /** Multi-field search panel. When provided, the left filter panel renders one
   * labelled control per field (text or exact-match select) instead of the
   * single `filterColumn` input. */
  filterFields?: FilterField[];
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
  /** Row density (drives cell vertical padding). Default "comfortable". */
  density?: Density;
}

export function DataTable<TData, TValue>({
  columns, data, filterColumn, filterPlaceholder, filterFields,
  emptyMessage = "No results.",
  toolbar, pageSize: initialPageSize = 10,
  getRowId, onRowClick, selectedRowId,
  filterOpen: filterOpenProp, onFilterOpenChange,
  columnVisibility, onColumnVisibilityChange,
  density = "comfortable",
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

  // Either a single-column input (legacy) or a multi-field panel enables filtering.
  const fields: FilterField[] =
    filterFields ?? (filterColumn ? [{ column: filterColumn, label: "Filter", placeholder: filterPlaceholder }] : []);
  const hasFilter = fields.length > 0;
  const activeFilterCount = fields.filter(
    (f) => (table.getColumn(f.column)?.getFilterValue() ?? "") !== "",
  ).length;
  const clearAllFilters = () =>
    fields.forEach((f) => table.getColumn(f.column)?.setFilterValue(undefined));

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
      {(toolbar || (hasFilter && !filterControlled)) && (
        <div className="flex shrink-0 items-center justify-end gap-2">
          {toolbar}
          {hasFilter && !filterControlled && (
            <TableFilterToggle open={filterOpen} onToggle={() => setFilterOpen(!filterOpen)} />
          )}
        </div>
      )}
      {/* Body: an optional left SEARCH PANEL (one control per field, expands from
          the left, mirroring the right-hand detail panel) beside the table. */}
      <div className="flex min-h-0 flex-1 gap-3">
        {hasFilter && filterOpen && (
          <aside className="bg-card animate-in slide-in-from-left-2 fade-in-0 flex w-64 shrink-0 flex-col rounded-xl border shadow-sm duration-200">
            <div className="flex items-center justify-between border-b px-3 py-2.5">
              <span className="flex items-center gap-2 text-sm font-semibold">
                Search
                {activeFilterCount > 0 && (
                  <Badge className="tabular-nums">{activeFilterCount}</Badge>
                )}
              </span>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Close search"
                onClick={() => setFilterOpen(false)}
              >
                <X className="size-4" />
              </Button>
            </div>
            <div className="min-h-0 flex-1 space-y-3 overflow-auto px-3 py-3">
              {fields.map((f, i) => {
                const col = table.getColumn(f.column);
                const value = (col?.getFilterValue() as string) ?? "";
                return (
                  <div key={f.column} className="space-y-1">
                    <Label
                      htmlFor={`filter-${f.column}`}
                      className="text-muted-foreground text-xs"
                    >
                      {f.label}
                    </Label>
                    {f.options ? (
                      <Select
                        value={value === "" ? "__any__" : value}
                        onValueChange={(v) => col?.setFilterValue(v === "__any__" ? undefined : v)}
                      >
                        <SelectTrigger id={`filter-${f.column}`} className="h-9 w-full text-sm">
                          <SelectValue placeholder="Any" />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="__any__">Any</SelectItem>
                          {f.options.map((o) => (
                            <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    ) : (
                      <Input
                        id={`filter-${f.column}`}
                        autoFocus={i === 0}
                        placeholder={f.placeholder ?? `Search ${f.label.toLowerCase()}…`}
                        value={value}
                        onChange={(e) => col?.setFilterValue(e.target.value || undefined)}
                      />
                    )}
                  </div>
                );
              })}
            </div>
            <div className="border-t px-3 py-2">
              <Button
                variant="ghost"
                size="sm"
                className="w-full"
                disabled={activeFilterCount === 0}
                onClick={clearAllFilters}
              >
                Clear all
              </Button>
            </div>
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
                        <Button
                          variant="ghost"
                          size="sm"
                          className="-ml-2.5 h-7 gap-1 px-2.5 text-xs font-semibold tracking-wide text-muted-foreground uppercase"
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
                        </Button>
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
                      <TableCell key={cell.id} className={`px-3 ${DENSITY_PADDING[density]}`}>
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
