import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check, X } from "lucide-react";
import { useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { useNavigate } from "react-router";
import { z } from "zod";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import { FormError } from "@/components/people/states";
import { UserAvatar } from "@/components/people/UserAvatar";
import { SLUG_HINT, SLUG_RE, slugify } from "@/components/people/validation";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { tenantPath } from "@/lib/paths";
import { useToast } from "@/toast/store";

const schema = z.object({
  name: z.string().trim().min(1, "Required.").max(100, "At most 100 characters."),
  slug: z.string().regex(SLUG_RE, SLUG_HINT),
});

type Values = z.infer<typeof schema>;

const EMPTY: Values = { name: "", slug: "" };

function SlugPreview({ slug }: { slug: string }) {
  const valid = SLUG_RE.test(slug);
  const Icon = valid ? Check : X;
  return (
    <div aria-live="polite" className="flex items-center gap-3 rounded-md border bg-muted/50 p-3">
      <UserAvatar name={slug || "?"} square />
      <div className="min-w-0 text-sm">
        <p className="flex items-center gap-1.5 font-medium">
          <Icon aria-hidden="true" className="size-3.5" />
          {slug ? (valid ? "Slug is valid" : "Slug is not valid") : "Enter a slug"}
        </p>
        <p className="truncate font-mono text-xs text-muted-foreground">
          {(slug || "slug") + "/<tree>"}
        </p>
      </div>
    </div>
  );
}

export function CreateTenantDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  // Until the slug is edited by hand it follows the name.
  const [slugEdited, setSlugEdited] = useState(false);

  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: EMPTY });
  const slug = useWatch({ control: form.control, name: "slug" });

  const create = useMutation({
    mutationFn: (v: Values) => api.tenants.create({ slug: v.slug, name: v.name }),
    meta: { inlineError: true },
    onSuccess: async (tenant) => {
      toast.success("Organisation created", tenant.slug);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.tenants }),
        queryClient.invalidateQueries({ queryKey: keys.me }),
      ]);
      changeOpen(false);
      void navigate(tenantPath(tenant.slug));
    },
  });

  function changeOpen(next: boolean) {
    if (!next) {
      form.reset(EMPTY);
      setSlugEdited(false);
      create.reset();
    }
    onOpenChange(next);
  }

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New organisation</DialogTitle>
          <DialogDescription>Organisations group trees and members. You become its owner.</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={(e) => void form.handleSubmit((v) => create.mutate(v))(e)} className="flex flex-col gap-4" noValidate>
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Display name</FormLabel>
                  <FormControl>
                    <Input
                      placeholder="Acme Corp"
                      autoComplete="organization"
                      {...field}
                      onChange={(e) => {
                        field.onChange(e);
                        if (!slugEdited) {
                          form.setValue("slug", slugify(e.target.value), { shouldValidate: form.formState.isSubmitted });
                        }
                      }}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="slug"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Slug</FormLabel>
                  <FormControl>
                    <Input
                      placeholder="acme"
                      autoComplete="off"
                      spellCheck={false}
                      className="font-mono"
                      {...field}
                      onChange={(e) => {
                        setSlugEdited(true);
                        field.onChange(e.target.value.toLowerCase());
                      }}
                    />
                  </FormControl>
                  <FormDescription>Used in URLs and remotes. {SLUG_HINT}</FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <SlugPreview slug={slug} />
            <FormError error={create.error} />
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="outline">
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" disabled={create.isPending}>
                {create.isPending ? "Creating…" : "Create organisation"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
