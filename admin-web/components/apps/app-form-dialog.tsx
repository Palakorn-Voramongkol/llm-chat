"use client";
import { useEffect } from "react";
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
import type { OidcApp, AppSecret } from "@/lib/types";
import {
  APP_TYPES, AUTH_METHODS, appToConfigForm, formToConfigBody, type ConfigForm,
} from "@/lib/oidc";
import { clientsBase, clientPath } from "@/lib/clients";

const schema = z.object({
  name: z.string().min(1),
  redirectUris: z.string().min(1, "at least one redirect URI"),
  appType: z.enum(["OIDC_APP_TYPE_WEB", "OIDC_APP_TYPE_NATIVE", "OIDC_APP_TYPE_USER_AGENT"]),
  authMethodType: z.enum([
    "OIDC_AUTH_METHOD_TYPE_BASIC", "OIDC_AUTH_METHOD_TYPE_POST",
    "OIDC_AUTH_METHOD_TYPE_NONE", "OIDC_AUTH_METHOD_TYPE_PRIVATE_KEY_JWT",
  ]),
});
type FormShape = z.infer<typeof schema>;

const DEFAULTS = {
  responseTypes: ["OIDC_RESPONSE_TYPE_CODE"] as const,
  grantTypes: ["OIDC_GRANT_TYPE_AUTHORIZATION_CODE", "OIDC_GRANT_TYPE_REFRESH_TOKEN"] as const,
};

// One dialog serves both modes. `mode` (not `!!app`) decides create-vs-edit so
// the page's edit instance — which sits at `app={editTarget}` and is therefore
// `null` whenever no row is selected — never falls into the create branch and
// renders a duplicate "Create application" trigger. The create instance owns
// its trigger; the edit instance is fully controlled by the page (mirrors the
// users/ EditUserDialog vs CreateUserDialog split).
export function AppFormDialog({
  mode, projectId, app, open, onOpenChange, onSaved, onSecret,
}: {
  mode: "create" | "edit";
  projectId: string;
  app: OidcApp | null;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onSaved: () => void;
  onSecret: (s: AppSecret) => void;
}) {
  const isEdit = mode === "edit";
  const form = useForm<FormShape>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: "", redirectUris: "",
      appType: "OIDC_APP_TYPE_WEB", authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC",
    },
  });
  useEffect(() => {
    if (app) {
      const f = appToConfigForm(app);
      form.reset({ name: f.name, redirectUris: f.redirectUris, appType: f.appType, authMethodType: f.authMethodType });
    } else {
      form.reset({ name: "", redirectUris: "", appType: "OIDC_APP_TYPE_WEB", authMethodType: "OIDC_AUTH_METHOD_TYPE_BASIC" });
    }
  }, [app, form]);

  async function onSubmit(values: FormShape) {
    const cfg: ConfigForm = {
      name: values.name,
      redirectUris: values.redirectUris,
      responseTypes: [...DEFAULTS.responseTypes],
      grantTypes: [...DEFAULTS.grantTypes],
      appType: values.appType,
      authMethodType: values.authMethodType,
    };
    const body = formToConfigBody(cfg);
    try {
      if (isEdit && app) {
        // read-modify-write: PUT the whole oidc_config (design §8).
        await api.put(clientPath(projectId, app.id), {
          redirectUris: body.redirectUris,
          responseTypes: body.responseTypes,
          grantTypes: body.grantTypes,
          appType: body.appType,
          authMethodType: body.authMethodType,
        });
        toast.success("Login client updated");
      } else {
        const created = await api.post<AppSecret>(clientsBase(projectId), body);
        toast.success("Login client created");
        if (created?.clientSecret) onSecret(created); // one-time reveal
      }
      onOpenChange(false);
      onSaved();
    } catch (e) {
      toast.error(e instanceof ApiError ? e.message : "Save failed");
    }
  }

  const inner = (
    <DialogContent>
      <DialogHeader><DialogTitle>{isEdit ? "Edit login client" : "Register login client"}</DialogTitle></DialogHeader>
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
          <FormField control={form.control} name="name" render={({ field }) => (
            <FormItem><FormLabel>Name</FormLabel>
              <FormControl><Input {...field} disabled={isEdit} /></FormControl><FormMessage /></FormItem>
          )} />
          <FormField control={form.control} name="redirectUris" render={({ field }) => (
            <FormItem><FormLabel>Redirect URIs (one per line)</FormLabel>
              <FormControl>
                <textarea
                  className="flex min-h-24 w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm"
                  {...field}
                />
              </FormControl><FormMessage /></FormItem>
          )} />
          <FormField control={form.control} name="appType" render={({ field }) => (
            <FormItem><FormLabel>App type</FormLabel>
              <Select onValueChange={field.onChange} value={field.value}>
                <FormControl><SelectTrigger><SelectValue /></SelectTrigger></FormControl>
                <SelectContent>
                  {APP_TYPES.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
                </SelectContent>
              </Select><FormMessage /></FormItem>
          )} />
          <FormField control={form.control} name="authMethodType" render={({ field }) => (
            <FormItem><FormLabel>Auth method</FormLabel>
              <Select onValueChange={field.onChange} value={field.value}>
                <FormControl><SelectTrigger><SelectValue /></SelectTrigger></FormControl>
                <SelectContent>
                  {AUTH_METHODS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}
                </SelectContent>
              </Select><FormMessage /></FormItem>
          )} />
          <DialogFooter><Button type="submit">{isEdit ? "Save" : "Create"}</Button></DialogFooter>
        </form>
      </Form>
    </DialogContent>
  );

  // create mode owns its trigger button; edit mode is controlled by the page.
  if (isEdit) {
    return <Dialog open={open} onOpenChange={onOpenChange}>{inner}</Dialog>;
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogTrigger asChild>
        <Button variant="brand" data-testid="create-app">Register login client</Button>
      </DialogTrigger>
      {inner}
    </Dialog>
  );
}
