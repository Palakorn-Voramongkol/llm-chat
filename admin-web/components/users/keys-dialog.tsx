"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Copy, Download, KeyRound, Plus, Trash2 } from "lucide-react";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter,
  DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { api, ApiError } from "@/lib/api";
import type { CreateKeyResponse, MachineKey, MachineKeyList, User } from "@/lib/types";

// Decode Zitadel's base64 `keyDetails` into the usable serviceaccount JSON file
// (the <user>-key.json shape the python/rust clients read). UTF-8 safe.
function decodeKeyDetails(b64: string): string {
  try {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const text = new TextDecoder().decode(bytes);
    try { return JSON.stringify(JSON.parse(text), null, 2); } catch { return text; }
  } catch {
    return b64;
  }
}

/** Manage a machine user's service-account JSON keys: list, generate (one-time
 * reveal of the private key), and revoke. This is how an app server is granted
 * credentials to call the chat service M2M (it still needs the chat.user role —
 * managed via the Access (grants) action). */
export function KeysDialog({
  user, open, onOpenChange,
}: {
  user: User | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const [keys, setKeys] = useState<MachineKey[] | null>(null);
  const [busy, setBusy] = useState(false);
  // The freshly-minted key file, shown ONCE (never re-fetchable). Cleared on close.
  const [revealed, setRevealed] =
    useState<{ json: string; fileName: string } | null>(null);

  const load = useCallback(async () => {
    if (!user) return;
    try {
      const l = await api.get<MachineKeyList>(`/api/users/${user.id}/keys`);
      setKeys(l.result ?? []);
    } catch {
      setKeys([]);
    }
  }, [user]);

  useEffect(() => {
    if (open && user) {
      setKeys(null);
      setRevealed(null);
      load();
    }
  }, [open, user, load]);

  async function generate() {
    if (!user) return;
    setBusy(true);
    try {
      const r = await api.post<CreateKeyResponse>(`/api/users/${user.id}/keys`);
      setRevealed({ json: decodeKeyDetails(r.keyDetails), fileName: `${user.userName}-key.json` });
      toast.success("Key generated — download it now");
      load();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Generate key failed");
    } finally {
      setBusy(false);
    }
  }

  async function revoke(keyId?: string) {
    if (!user || !keyId) return;
    try {
      await api.del(`/api/users/${user.id}/keys/${keyId}`);
      toast.success("Key revoked");
      load();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Revoke failed");
    }
  }

  function download() {
    if (!revealed) return;
    const url = URL.createObjectURL(new Blob([revealed.json], { type: "application/json" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = revealed.fileName;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function copy() {
    if (!revealed) return;
    try {
      await navigator.clipboard.writeText(revealed.json);
      toast.success("Copied");
    } catch {
      toast.error("Copy failed — select and copy manually");
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="size-4 text-indigo-600" />
            Credentials — {user?.userName}
          </DialogTitle>
          <DialogDescription>
            Service-account JSON keys for machine-to-machine access. A key lets an
            app server mint a token and call the chat service (it still needs the
            chat.user role — see Access (grants)).
          </DialogDescription>
        </DialogHeader>

        {revealed ? (
          <div className="space-y-3" data-testid="key-reveal">
            <div className="rounded-md border border-amber-300 bg-amber-50 p-3 text-xs text-amber-900 dark:border-amber-900/50 dark:bg-amber-950/40 dark:text-amber-200">
              <p className="font-medium">Shown once — save it now.</p>
              <p>This private key is not stored and cannot be retrieved again. Download or copy it before closing.</p>
            </div>
            <pre className="bg-muted max-h-52 overflow-auto rounded-md border p-3 font-mono text-[11px] whitespace-pre">
              {revealed.json}
            </pre>
            <div className="flex gap-2">
              <Button onClick={download} data-testid="key-download">
                <Download className="size-4" />
                Download {revealed.fileName}
              </Button>
              <Button variant="outline" onClick={copy}>
                <Copy className="size-4" />
                Copy
              </Button>
            </div>
            <DialogFooter>
              <Button variant="ghost" data-testid="key-reveal-done" onClick={() => setRevealed(null)}>
                Done
              </Button>
            </DialogFooter>
          </div>
        ) : (
          <div className="space-y-3">
            <div className="rounded-xl border">
              {keys === null ? (
                <p className="text-muted-foreground p-3 text-sm">Loading…</p>
              ) : keys.length === 0 ? (
                <p className="text-muted-foreground p-3 text-sm">No keys yet.</p>
              ) : (
                <ul className="divide-y">
                  {keys.map((k) => (
                    <li key={k.id} className="flex items-center gap-2 px-3 py-2 text-sm">
                      <KeyRound className="text-muted-foreground size-3.5 shrink-0" />
                      <span className="font-mono text-xs">{k.id}</span>
                      <span className="text-muted-foreground ml-auto text-xs whitespace-nowrap">
                        {k.expirationDate
                          ? `expires ${new Date(k.expirationDate).toLocaleDateString()}`
                          : "no expiry"}
                      </span>
                      <Button variant="ghost" size="sm"
                        className="text-destructive size-7 shrink-0 p-0"
                        aria-label="Revoke key" data-testid="key-revoke"
                        onClick={() => revoke(k.id)}>
                        <Trash2 className="size-3.5" />
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <Button variant="brand" disabled={busy} data-testid="key-generate" onClick={generate}>
              <Plus className="size-4" />
              Generate new key
            </Button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
