"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { File, Folder, FilePlus, FolderPlus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogTrigger,
} from "@/components/ui/dialog";
import { api, ApiError } from "@/lib/api";
import { entriesToTree, isValidPath, type TemplateEntry, type TplNode } from "@/lib/sandbox-template";

const VARS = "{{name}}  {{userId}}  {{app}}  {{date}}";

/** Author a login client's versioned sandbox template (two-pane tree + content
 * editor). A plain Save edits the current version's content; Publish bumps the
 * version and REQUIRES migration instructions. The server is authoritative for
 * the version and path confinement; client-side checks are fast feedback only. */
export function SandboxTemplateEditor({ pid, appId }: { pid: string; appId: string }) {
  const [entries, setEntries] = useState<TemplateEntry[]>([]);
  const [version, setVersion] = useState(0);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [publishOpen, setPublishOpen] = useState(false);
  const [instructions, setInstructions] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const t = await api.getSandboxTemplate(pid, appId);
      if (!t.configured) {
        setLoadError("Sandbox templates are not configured on this server.");
        return;
      }
      setVersion(t.version);
      const list = (t.template ?? []).map((e) => ({ path: e.path, dir: e.dir, content: e.content }));
      setEntries(list);
      setSelected(list.find((e) => !e.dir)?.path ?? list[0]?.path ?? null);
      setLoadError(t.ok ? null : t.error ?? "Failed to load template");
    } catch (e) {
      setLoadError(e instanceof ApiError ? e.message : "Failed to load template");
    } finally {
      setLoading(false);
    }
  }, [pid, appId]);

  useEffect(() => { void load(); }, [load]);

  const tree = useMemo(() => entriesToTree(entries), [entries]);
  const selectedEntry = entries.find((e) => e.path === selected) ?? null;
  const firstInvalid = entries.find((e) => !isValidPath(e.path));

  const setContent = (path: string, content: string) =>
    setEntries((es) => es.map((e) => (e.path === path ? { ...e, content } : e)));

  const setPath = (oldPath: string, newPath: string) =>
    setEntries((es) => es.map((e) => {
      if (e.path === oldPath) return { ...e, path: newPath };
      // rewrite descendants when renaming a folder
      if (e.path.startsWith(oldPath + "/")) return { ...e, path: newPath + e.path.slice(oldPath.length) };
      return e;
    }));

  const addEntry = (dir: boolean) => {
    const base = dir ? "new-folder" : "new-file.md";
    let path = base;
    let n = 1;
    while (entries.some((e) => e.path === path)) {
      path = dir ? `new-folder-${n}` : `new-file-${n}.md`;
      n += 1;
    }
    setEntries((es) => [...es, { path, dir, content: "" }]);
    setSelected(path);
  };

  const deleteEntry = (path: string) => {
    setEntries((es) => es.filter((e) => e.path !== path && !e.path.startsWith(path + "/")));
    if (selected === path || selected?.startsWith(path + "/")) setSelected(null);
  };

  const save = async (publish: boolean) => {
    if (firstInvalid) {
      toast.error(`Invalid path: ${firstInvalid.path}`);
      return;
    }
    setSaving(true);
    try {
      const res = await api.saveSandboxTemplate(pid, appId, {
        template: entries,
        publish,
        migrateInstructions: publish ? instructions : undefined,
      });
      if (!res.ok) {
        toast.error(res.error ?? "Save failed");
        return;
      }
      setVersion(res.version);
      setPublishOpen(false);
      setInstructions("");
      toast.success(publish ? `Published v${res.version}` : "Template saved");
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Save failed");
    } finally {
      setSaving(false);
    }
  };

  if (loading) return <p className="text-muted-foreground text-sm">Loading template…</p>;
  if (loadError) return <p className="text-destructive text-sm">{loadError}</p>;

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="secondary">v{version}</Badge>
        <span className="text-muted-foreground text-xs">Variables: <span className="font-mono">{VARS}</span></span>
        <div className="ml-auto flex gap-2">
          <Button size="sm" variant="outline" onClick={() => save(false)} disabled={saving}>Save</Button>
          <Dialog open={publishOpen} onOpenChange={setPublishOpen}>
            <DialogTrigger asChild>
              <Button size="sm" variant="brand" disabled={saving}>Publish new version</Button>
            </DialogTrigger>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Publish new version (v{version + 1})</DialogTitle>
              </DialogHeader>
              <p className="text-muted-foreground text-sm">
                Publishing migrates every returning user&apos;s sandbox via the LLM.
                Describe how to bring an older box up to this version.
              </p>
              <Label htmlFor="migrate-instructions">Migration instructions</Label>
              <Textarea id="migrate-instructions" rows={6} value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
                placeholder="e.g. Rename config.json to settings.json, keeping the user's values…" />
              <DialogFooter>
                <Button variant="outline" onClick={() => setPublishOpen(false)}>Cancel</Button>
                <Button variant="brand" disabled={saving || !instructions.trim()} onClick={() => save(true)}>
                  Publish
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-[minmax(0,14rem)_1fr]">
        <div className="rounded-md border">
          <div className="flex items-center gap-1 border-b p-1.5">
            <Button size="sm" variant="ghost" className="h-7 px-2" onClick={() => addEntry(false)}>
              <FilePlus className="size-3.5" /> File
            </Button>
            <Button size="sm" variant="ghost" className="h-7 px-2" onClick={() => addEntry(true)}>
              <FolderPlus className="size-3.5" /> Folder
            </Button>
          </div>
          {entries.length === 0 ? (
            <p className="text-muted-foreground p-3 text-sm">Empty template.</p>
          ) : (
            <ul className="p-1.5">
              <TreeLevel nodes={tree} depth={0} selected={selected} onSelect={setSelected} />
            </ul>
          )}
        </div>

        <div className="space-y-2">
          {selectedEntry ? (
            <>
              <div className="flex items-end gap-2">
                <div className="flex-1">
                  <Label htmlFor="entry-path">Path</Label>
                  <Input id="entry-path" value={selectedEntry.path}
                    aria-invalid={!isValidPath(selectedEntry.path)}
                    onChange={(e) => setPath(selectedEntry.path, e.target.value)} />
                </div>
                <Button size="sm" variant="outline" className="text-destructive"
                  onClick={() => deleteEntry(selectedEntry.path)}>
                  <Trash2 className="size-3.5" /> Delete
                </Button>
              </div>
              {!isValidPath(selectedEntry.path) && (
                <p className="text-destructive text-xs">Invalid path (no .., absolute, \, :, or empty segments).</p>
              )}
              {!selectedEntry.dir && (
                <div>
                  <Label htmlFor="entry-content">Content</Label>
                  <Textarea id="entry-content" rows={14} className="font-mono text-xs"
                    value={selectedEntry.content}
                    onChange={(e) => setContent(selectedEntry.path, e.target.value)} />
                </div>
              )}
            </>
          ) : (
            <p className="text-muted-foreground text-sm">Select a file or folder to edit.</p>
          )}
        </div>
      </div>
    </div>
  );
}

function TreeLevel({
  nodes, depth, selected, onSelect,
}: {
  nodes: TplNode[];
  depth: number;
  selected: string | null;
  onSelect: (path: string) => void;
}) {
  return (
    <>
      {nodes.map((n) => (
        <li key={n.path}>
          <button type="button" aria-pressed={selected === n.path}
            onClick={() => onSelect(n.path)}
            style={{ paddingLeft: `${depth * 0.85 + 0.4}rem` }}
            className={`flex w-full items-center gap-1.5 rounded px-1.5 py-1 text-left text-sm transition-colors ${
              selected === n.path ? "bg-muted font-medium" : "hover:bg-muted/50"
            }`}>
            {n.dir ? <Folder className="size-3.5 shrink-0 text-amber-500" />
                   : <File className="size-3.5 shrink-0 text-muted-foreground" />}
            <span className="truncate">{n.name}</span>
          </button>
          {n.dir && n.children.length > 0 && (
            <ul>
              <TreeLevel nodes={n.children} depth={depth + 1} selected={selected} onSelect={onSelect} />
            </ul>
          )}
        </li>
      ))}
    </>
  );
}
