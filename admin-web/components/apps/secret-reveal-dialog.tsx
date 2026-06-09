"use client";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

// One-time secret reveal (design §3 invariant). The secret is held only in the
// caller's state from the create/regenerate response and is NEVER refetched;
// dismissing this dialog discards it. No logging.
export function SecretRevealDialog({
  clientId, clientSecret, onClose,
}: {
  clientId?: string;
  clientSecret: string | null;
  onClose: () => void;
}) {
  async function copy() {
    try {
      await navigator.clipboard.writeText(clientSecret ?? "");
      toast.success("Copied to clipboard");
    } catch {
      toast.error("Copy failed — select and copy manually");
    }
  }
  return (
    <Dialog open={!!clientSecret} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Client secret — copy it now</DialogTitle>
          <DialogDescription>
            This secret is shown once and cannot be retrieved again. Copy and
            store it before closing this dialog.
          </DialogDescription>
        </DialogHeader>
        {clientId && (
          <div className="space-y-1">
            <p className="text-sm text-muted-foreground">Client ID</p>
            <Input readOnly value={clientId} data-testid="reveal-client-id" />
          </div>
        )}
        <div className="space-y-1">
          <p className="text-sm text-muted-foreground">Client secret</p>
          <Input readOnly value={clientSecret ?? ""} data-testid="reveal-client-secret" />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={copy} data-testid="reveal-copy">Copy</Button>
          <Button onClick={onClose} data-testid="reveal-done">Done</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
