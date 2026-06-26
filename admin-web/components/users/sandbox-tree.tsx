"use client";
import { useState } from "react";
import { ChevronRight, ChevronDown, Folder, File as FileIcon } from "lucide-react";
import type { TreeNode } from "@/lib/sandbox-tree";

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function Node({ node, depth }: { node: TreeNode; depth: number }) {
  const [open, setOpen] = useState(depth < 1); // top level expanded
  if (node.dir) {
    return (
      <div>
        <button type="button" onClick={() => setOpen((o) => !o)}
          className="hover:bg-muted/50 flex w-full items-center gap-1.5 rounded px-1 py-0.5 text-left text-sm"
          style={{ paddingLeft: `${depth * 14}px` }}>
          {open ? <ChevronDown className="size-3.5 shrink-0" /> : <ChevronRight className="size-3.5 shrink-0" />}
          <Folder className="size-4 shrink-0 text-sky-600" />
          <span className="truncate">{node.name}</span>
        </button>
        {open && node.children.map((c) => <Node key={c.path} node={c} depth={depth + 1} />)}
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1.5 px-1 py-0.5 text-sm"
      style={{ paddingLeft: `${depth * 14 + 19}px` }}>
      <FileIcon className="text-muted-foreground size-4 shrink-0" />
      <span className="truncate">{node.name}</span>
      <span className="text-muted-foreground ml-auto text-xs tabular-nums">{fmtSize(node.size)}</span>
    </div>
  );
}

export function SandboxTree({ nodes }: { nodes: TreeNode[] }) {
  return <div className="space-y-0.5">{nodes.map((n) => <Node key={n.path} node={n} depth={0} />)}</div>;
}
