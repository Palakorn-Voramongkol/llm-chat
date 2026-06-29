export interface TemplateEntry {
  path: string;
  dir: boolean;
  content: string;
}

export interface TplNode {
  name: string;
  path: string;
  dir: boolean;
  content: string;
  children: TplNode[];
}

// Build a nested editable tree. Intermediate folders implied by a file path are
// materialized as structural dir nodes; an explicit dir entry maps to a dir
// node too. Folders sort before files, then alphabetical.
export function entriesToTree(entries: TemplateEntry[]): TplNode[] {
  const roots: TplNode[] = [];
  const byPath = new Map<string, TplNode>();
  const ensureDir = (path: string): TplNode => {
    const existing = byPath.get(path);
    if (existing) return existing;
    const segs = path.split("/");
    const node: TplNode = { name: segs[segs.length - 1], path, dir: true, content: "", children: [] };
    byPath.set(path, node);
    const parent = segs.length > 1 ? ensureDir(segs.slice(0, -1).join("/")) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
    return node;
  };
  const sorted = [...entries].sort((a, b) => a.path.localeCompare(b.path));
  for (const e of sorted) {
    const segs = e.path.split("/");
    if (e.dir) {
      ensureDir(e.path);
      continue;
    }
    const node: TplNode = { name: segs[segs.length - 1], path: e.path, dir: false, content: e.content, children: [] };
    byPath.set(e.path, node);
    const parent = segs.length > 1 ? ensureDir(segs.slice(0, -1).join("/")) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
  }
  sortLevel(roots);
  return roots;
}

// Flatten back to entries. Files always emit; a dir emits an explicit entry
// ONLY when it has no descendants (an empty folder) — intermediate folders are
// implied by their files (matches the worker's provision_entries).
export function treeToEntries(roots: TplNode[]): TemplateEntry[] {
  const out: TemplateEntry[] = [];
  const walk = (node: TplNode): void => {
    if (!node.dir) {
      out.push({ path: node.path, dir: false, content: node.content });
      return;
    }
    if (node.children.length === 0) {
      out.push({ path: node.path, dir: true, content: "" });
      return;
    }
    for (const c of node.children) walk(c);
  };
  for (const r of roots) walk(r);
  return out;
}

function sortLevel(nodes: TplNode[]): void {
  nodes.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
  for (const n of nodes) sortLevel(n.children);
}

// Mirror the worker's confine_path rules (fast client feedback; the server is
// authoritative). Reject empty, absolute, traversal, '.', '\\', ':', NUL.
export function isValidPath(path: string): boolean {
  if (!path || path.startsWith("/")) return false;
  for (const seg of path.split("/")) {
    if (seg === "" || seg === "." || seg === "..") return false;
    if (seg.includes("\\") || seg.includes(":") || seg.includes("\0")) return false;
  }
  return true;
}
