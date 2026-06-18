"use client";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogFooter,
} from "@/components/ui/dialog";
import {
  Form, FormControl, FormField, FormItem, FormLabel, FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, ApiError } from "@/lib/api";
import type { Role } from "@/lib/types";

const schema = z.object({
  displayName: z.string().min(1),
  group: z.string().optional(),
});
type FormValues = z.infer<typeof schema>;

/** Edit a role's display name + group. The role KEY is its immutable id and is
 * shown disabled — the backend ignores a changed key, so it can't be renamed.
 * `endpoint` is the PUT path so this works for both the home project
 * (/api/roles/{key}) and any application (/api/projects/{pid}/roles/{key}). */
export function RoleEditDialog({
  role, endpoint, open, onOpenChange, onSaved,
}: {
  role: Role | null;
  endpoint: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
}) {
  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { displayName: "", group: "" },
  });

  // Pre-fill from the role each time it changes (or the dialog re-opens).
  useEffect(() => {
    if (role) {
      form.reset({ displayName: role.displayName ?? "", group: role.group ?? "" });
    }
  }, [role, form]);

  async function onSubmit(values: FormValues) {
    try {
      await api.put(endpoint, {
        displayName: values.displayName,
        group: values.group ?? "",
      });
      toast.success("Role updated");
      onSaved();
      onOpenChange(false);
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit role</DialogTitle>
          <DialogDescription>
            Change the display name and group. The role key is permanent.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <div className="grid gap-2">
              <Label htmlFor="role-edit-key">Role key</Label>
              <Input
                id="role-edit-key"
                value={role?.key ?? ""}
                disabled
                readOnly
                className="font-mono"
              />
              <p className="text-muted-foreground text-sm">
                The role key is permanent and can&apos;t be changed.
              </p>
            </div>
            <FormField control={form.control} name="displayName" render={({ field }) => (
              <FormItem><FormLabel>Display name</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="group" render={({ field }) => (
              <FormItem><FormLabel>Group (optional)</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <DialogFooter>
              <Button type="submit" data-testid="role-edit-save">Save</Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
