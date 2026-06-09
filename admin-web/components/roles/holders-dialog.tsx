"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { api, ApiError } from "@/lib/api";
import type { Role, RoleHolder, RoleHolderList } from "@/lib/types";

export function HoldersDialog({
  role, open, onOpenChange,
}: {
  role: Role | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const [holders, setHolders] = useState<RoleHolder[]>([]);

  const load = useCallback(async () => {
    if (!role) return;
    try {
      // roleKey is part of the path -> encode (design §7).
      const list = await api.get<RoleHolderList>(
        `/api/roles/${encodeURIComponent(role.key)}/holders`,
      );
      setHolders(list.result);
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error(e instanceof ApiError ? e.message : "Failed to load holders");
      }
    }
  }, [role]);

  useEffect(() => {
    if (open) load();
    else setHolders([]);
  }, [open, load]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Holders of {role?.key}</DialogTitle>
        </DialogHeader>
        {holders.length === 0 ? (
          <p className="text-sm text-muted-foreground">No one holds this role.</p>
        ) : (
          <ul className="space-y-2">
            {holders.map((h) => (
              <li key={h.id} className="flex items-center justify-between gap-2">
                <span className="text-sm">{h.displayName ?? h.userName ?? h.userId}</span>
                <Badge variant="secondary">{h.userId}</Badge>
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}
