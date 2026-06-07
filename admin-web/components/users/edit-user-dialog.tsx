"use client";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from "@/components/ui/dialog";
import {
  Form, FormControl, FormField, FormItem, FormLabel, FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { api, ApiError } from "@/lib/api";
import type { User } from "@/lib/types";

const schema = z.object({
  givenName: z.string().min(1),
  familyName: z.string().min(1),
  displayName: z.string().optional(),
});
type FormValues = z.infer<typeof schema>;

export function EditUserDialog({
  user, open, onOpenChange, onSaved,
}: {
  user: User | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
}) {
  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { givenName: "", familyName: "", displayName: "" },
  });
  useEffect(() => {
    const [given = "", family = ""] = (user?.displayName ?? "").split(" ");
    form.reset({ givenName: given, familyName: family, displayName: user?.displayName ?? "" });
  }, [user, form]);

  async function onSubmit(values: FormValues) {
    if (!user) return;
    try {
      await api.patch(`/api/users/${user.id}/profile`, values);
      toast.success("Profile updated");
      onOpenChange(false);
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Update failed");
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>Edit profile</DialogTitle></DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <FormField control={form.control} name="givenName" render={({ field }) => (
              <FormItem><FormLabel>Given name</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="familyName" render={({ field }) => (
              <FormItem><FormLabel>Family name</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <DialogFooter><Button type="submit">Save</Button></DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
