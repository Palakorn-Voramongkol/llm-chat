"use client";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger, DialogFooter,
} from "@/components/ui/dialog";
import {
  Form, FormControl, FormField, FormItem, FormLabel, FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { api, ApiError } from "@/lib/api";

const schema = z.object({
  roleKey: z.string().min(1),
  displayName: z.string().min(1),
  group: z.string().optional(),
});
type FormValues = z.infer<typeof schema>;

export function CreateRoleDialog({ onCreated }: { onCreated: () => void }) {
  const [open, setOpen] = useState(false);
  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { roleKey: "", displayName: "", group: "" },
  });

  async function onSubmit(values: FormValues) {
    try {
      await api.post("/api/roles", {
        roleKey: values.roleKey,
        displayName: values.displayName,
        group: values.group ?? "",
      });
      toast.success("Role created");
      setOpen(false);
      form.reset();
      onCreated();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Create failed");
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button data-testid="create-role">Create role</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>Create role</DialogTitle></DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <FormField control={form.control} name="roleKey" render={({ field }) => (
              <FormItem><FormLabel>Role key</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="displayName" render={({ field }) => (
              <FormItem><FormLabel>Display name</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <FormField control={form.control} name="group" render={({ field }) => (
              <FormItem><FormLabel>Group (optional)</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            <DialogFooter><Button type="submit">Create</Button></DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
