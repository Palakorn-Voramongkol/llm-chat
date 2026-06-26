import type { SandboxEntry } from "@/lib/types";

export interface TreeNode {
  name: string;          // last path segment
  path: string;          // full relative path
  dir: boolean;
  size: number;          // 0 for directories
  children: TreeNode[];
}

// Turn the flat, '/'-separated entry list into a nested tree. Folders are
// placed before files at each level, then alphabetical. The worker already
// sorts entries so a parent dir precedes its children; we sort defensively too.
export function buildTree(entries: SandboxEntry[]): TreeNode[] {
  const roots: TreeNode[] = [];
  const byPath = new Map<string, TreeNode>();
  const sorted = [...entries].sort((a, b) => a.path.localeCompare(b.path));
  for (const e of sorted) {
    const segs = e.path.split("/");
    const node: TreeNode = {
      name: segs[segs.length - 1],
      path: e.path,
      dir: e.dir,
      size: e.dir ? 0 : e.size,
      children: [],
    };
    byPath.set(e.path, node);
    const parent = segs.length > 1 ? byPath.get(segs.slice(0, -1).join("/")) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node); // top-level, or an orphan whose parent wasn't listed
  }
  sortLevel(roots);
  return roots;
}

function sortLevel(nodes: TreeNode[]): void {
  nodes.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
  for (const n of nodes) sortLevel(n.children);
}
