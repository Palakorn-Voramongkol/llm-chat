"use client";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { FolderKanban } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card, CardContent, CardDescription, CardHeader, CardTitle,
} from "@/components/ui/card";
import {
  Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { api, ApiError } from "@/lib/api";
import type { Project } from "@/lib/types";

const schema = z.object({
  name: z.string().min(1),
  projectRoleAssertion: z.boolean(),
  projectRoleCheck: z.boolean(),
  hasProjectCheck: z.boolean(),
});
type FormShape = z.infer<typeof schema>;

const EMPTY: FormShape = {
  name: "",
  projectRoleAssertion: false,
  projectRoleCheck: false,
  hasProjectCheck: false,
};

// Editable view of the platform project (design §9). Mirrors the app form's
// react-hook-form + zodResolver + api.put + toast/ApiError pattern. The Save
// button stays disabled until `project` loads so we never PUT an empty body.
export function ProjectCard({
  project, onSaved,
}: {
  project: Project | null;
  onSaved: () => void;
}) {
  const form = useForm<FormShape>({
    resolver: zodResolver(schema),
    defaultValues: EMPTY,
  });

  useEffect(() => {
    if (project) {
      form.reset({
        name: project.name,
        projectRoleAssertion: project.projectRoleAssertion ?? false,
        projectRoleCheck: project.projectRoleCheck ?? false,
        hasProjectCheck: project.hasProjectCheck ?? false,
      });
    } else {
      form.reset(EMPTY);
    }
  }, [project, form]);

  async function onSubmit(values: FormShape) {
    try {
      await api.put("/api/project", values);
      toast.success("Project updated");
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Save failed");
    }
  }

  return (
    <Card data-testid="project-card">
      <CardHeader>
        <div className="flex items-center gap-2.5">
          <span aria-hidden
            className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-indigo-500/10 text-indigo-600">
            <FolderKanban className="size-4" />
          </span>
          <CardTitle>Project</CardTitle>
        </div>
        <CardDescription>
          The platform project that owns every OIDC app and role grant.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-6">
            <FormField control={form.control} name="name" render={({ field }) => (
              <FormItem>
                <FormLabel>Name</FormLabel>
                <FormControl><Input data-testid="project-name" {...field} /></FormControl>
                <FormMessage />
              </FormItem>
            )} />
            <FormField control={form.control} name="projectRoleAssertion" render={({ field }) => (
              <FormItem className="flex flex-row items-center justify-between gap-4">
                <div className="space-y-1">
                  <FormLabel>Assert roles in tokens</FormLabel>
                  <FormDescription>Embed this project&apos;s role claims in issued tokens.</FormDescription>
                </div>
                <FormControl>
                  <Switch checked={field.value} onCheckedChange={field.onChange} />
                </FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="projectRoleCheck" render={({ field }) => (
              <FormItem className="flex flex-row items-center justify-between gap-4">
                <div className="space-y-1">
                  <FormLabel>Check for project roles</FormLabel>
                  <FormDescription>Require a granted project role to obtain a token.</FormDescription>
                </div>
                <FormControl>
                  <Switch checked={field.value} onCheckedChange={field.onChange} />
                </FormControl>
              </FormItem>
            )} />
            <FormField control={form.control} name="hasProjectCheck" render={({ field }) => (
              <FormItem className="flex flex-row items-center justify-between gap-4">
                <div className="space-y-1">
                  <FormLabel>Check for project grant</FormLabel>
                  <FormDescription>Require the user&apos;s org to hold a grant for this project.</FormDescription>
                </div>
                <FormControl>
                  <Switch checked={field.value} onCheckedChange={field.onChange} />
                </FormControl>
              </FormItem>
            )} />
            <Button type="submit" data-testid="project-save" disabled={!project}>Save</Button>
          </form>
        </Form>
      </CardContent>
    </Card>
  );
}
