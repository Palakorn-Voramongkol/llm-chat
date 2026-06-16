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
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { api, ApiError } from "@/lib/api";

const humanSchema = z.object({
  kind: z.literal("Human"),
  userName: z.string().min(1),
  givenName: z.string().min(1),
  familyName: z.string().min(1),
  email: z.string().email(),
  password: z.string().min(8).optional().or(z.literal("")),
});
const machineSchema = z.object({
  kind: z.literal("Machine"),
  userName: z.string().min(1),
  name: z.string().min(1),
});
const schema = z.discriminatedUnion("kind", [humanSchema, machineSchema]);
type FormValues = z.infer<typeof schema>;

export function CreateUserDialog({
  onCreated,
  open: openProp,
  onOpenChange,
}: {
  onCreated: () => void;
  /** Optional controlled open state — lift this so another control (e.g. the
   * filter panel's "+") can open the same dialog. Uncontrolled when omitted. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const [internalOpen, setInternalOpen] = useState(false);
  const open = openProp ?? internalOpen;
  const setOpen = (next: boolean) => {
    onOpenChange ? onOpenChange(next) : setInternalOpen(next);
  };
  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { kind: "Human", userName: "", givenName: "", familyName: "", email: "" },
  });
  const kind = form.watch("kind");

  async function onSubmit(values: FormValues) {
    try {
      if (values.kind === "Human") {
        await api.post("/api/users/human", {
          userName: values.userName,
          givenName: values.givenName,
          familyName: values.familyName,
          email: values.email,
          ...(values.password ? { password: values.password } : {}),
        });
      } else {
        await api.post("/api/users/machine", {
          userName: values.userName, name: values.name,
        });
      }
      toast.success("User created");
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
        <Button variant="brand" data-testid="create-user">Create user</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader><DialogTitle>Create user</DialogTitle></DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            <FormField control={form.control} name="kind" render={({ field }) => (
              <FormItem>
                <FormLabel>Type</FormLabel>
                <Select onValueChange={field.onChange} value={field.value}>
                  <FormControl><SelectTrigger><SelectValue /></SelectTrigger></FormControl>
                  <SelectContent>
                    <SelectItem value="Human">Human</SelectItem>
                    <SelectItem value="Machine">Machine</SelectItem>
                  </SelectContent>
                </Select>
                <FormMessage />
              </FormItem>
            )} />
            <FormField control={form.control} name="userName" render={({ field }) => (
              <FormItem><FormLabel>Username</FormLabel>
                <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
            )} />
            {kind === "Human" ? (
              <>
                <FormField control={form.control} name="givenName" render={({ field }) => (
                  <FormItem><FormLabel>Given name</FormLabel>
                    <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
                )} />
                <FormField control={form.control} name="familyName" render={({ field }) => (
                  <FormItem><FormLabel>Family name</FormLabel>
                    <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
                )} />
                <FormField control={form.control} name="email" render={({ field }) => (
                  <FormItem><FormLabel>Email</FormLabel>
                    <FormControl><Input type="email" {...field} /></FormControl><FormMessage /></FormItem>
                )} />
                <FormField control={form.control} name="password" render={({ field }) => (
                  <FormItem><FormLabel>Password (optional)</FormLabel>
                    <FormControl><Input type="password" {...field} /></FormControl><FormMessage /></FormItem>
                )} />
              </>
            ) : (
              <FormField control={form.control} name="name" render={({ field }) => (
                <FormItem><FormLabel>Display name</FormLabel>
                  <FormControl><Input {...field} /></FormControl><FormMessage /></FormItem>
              )} />
            )}
            <DialogFooter><Button type="submit">Create</Button></DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
