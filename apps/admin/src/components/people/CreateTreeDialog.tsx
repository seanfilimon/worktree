import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Globe, Lock, Users } from "lucide-react";
import { useId } from "react";
import { useForm, useWatch } from "react-hook-form";
import { useNavigate } from "react-router";
import { z } from "zod";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import { VISIBILITIES, type Tenant, type Visibility } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { FormError } from "./states";
import { TREE_NAME_HINT, TREE_NAME_RE } from "./validation";

const schema = z.object({
  tenant: z.string().min(1, "Pick a tenant."),
  name: z.string().trim().regex(TREE_NAME_RE, TREE_NAME_HINT),
  description: z.string().trim().max(500, "At most 500 characters."),
  visibility: z.enum(VISIBILITIES),
  default_branch: z.string().trim().min(1, "Required.").max(255, "At most 255 characters."),
});

type Values = z.infer<typeof schema>;

const VISIBILITY_HELP: Record<Visibility, { icon: typeof Lock; text: string }> = {
  private: { icon: Lock, text: "Only tenant members can read it." },
  internal: { icon: Users, text: "Any signed-in account can read it." },
  public: { icon: Globe, text: "Anyone, even signed out, can read it." },
};

interface CreateTreeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Tenants the caller may create trees in. One entry hides the picker. */
  tenants: Tenant[];
  defaultTenant?: string;
}

export function CreateTreeDialog({ open, onOpenChange, tenants, defaultTenant }: CreateTreeDialogProps) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const initialTenant = tenants.some((t) => t.slug === defaultTenant) ? defaultTenant! : (tenants[0]?.slug ?? "");

  const defaults = (): Values => ({ tenant: initialTenant, name: "", description: "", visibility: "private", default_branch: "main" });
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: defaults() });
  const selectedTenant = useWatch({ control: form.control, name: "tenant" });
  const visibilityLabelId = useId();

  const create = useMutation({
    mutationFn: ({ tenant, ...body }: Values) => api.tenants.createTree(tenant, body),
    meta: { inlineError: true },
    onSuccess: async (tree, { tenant }) => {
      toast.success("Tree created", `${tenant}/${tree.name}`);
      await queryClient.invalidateQueries({ queryKey: keys.tenantTrees(tenant) });
      form.reset(defaults());
      onOpenChange(false);
      void navigate(treePath(tenant, tree.name));
    },
  });

  const changeOpen = (next: boolean) => {
    if (!next) {
      form.reset(defaults());
      create.reset();
    }
    onOpenChange(next);
  };

  const single = tenants.length === 1;

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New tree</DialogTitle>
          <DialogDescription>
            {single ? (
              <>
                Create a tree in <span className="font-mono text-foreground">{tenants[0]!.slug}</span>.
              </>
            ) : (
              "Create an empty tree, then push to it with wt."
            )}
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={(e) => void form.handleSubmit((v) => create.mutate(v))(e)} className="flex flex-col gap-4" noValidate>
            {!single && (
              <FormField
                control={form.control}
                name="tenant"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Tenant</FormLabel>
                    <Select value={field.value} onValueChange={field.onChange}>
                      <FormControl>
                        <SelectTrigger className="w-full">
                          <SelectValue placeholder="Pick a tenant" />
                        </SelectTrigger>
                      </FormControl>
                      <SelectContent>
                        {tenants.map((t) => (
                          <SelectItem key={t.id} value={t.slug}>
                            {t.slug}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    <FormMessage />
                  </FormItem>
                )}
              />
            )}
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Name</FormLabel>
                  <FormControl>
                    <Input placeholder="backend" autoComplete="off" className="font-mono" {...field} />
                  </FormControl>
                  <FormDescription>
                    Remote: <span className="font-mono">{selectedTenant || "tenant"}/{field.value.trim() || "name"}</span>
                  </FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="description"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Description</FormLabel>
                  <FormControl>
                    <Textarea rows={2} placeholder="Optional" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="visibility"
              render={({ field }) => (
                <FormItem>
                  <p className="text-sm leading-none font-medium" id={visibilityLabelId}>
                    Visibility
                  </p>
                  <FormControl>
                    <RadioGroup aria-labelledby={visibilityLabelId} value={field.value} onValueChange={field.onChange} className="gap-2">
                      {VISIBILITIES.map((v) => {
                        const { icon: Icon, text } = VISIBILITY_HELP[v];
                        return (
                          <div key={v} className="flex items-start gap-3 rounded-md border p-3 has-[[data-state=checked]]:border-foreground">
                            <RadioGroupItem value={v} id={`${visibilityLabelId}-${v}`} className="mt-0.5" />
                            <Label htmlFor={`${visibilityLabelId}-${v}`} className="flex flex-1 flex-col items-start gap-1">
                              <span className="flex items-center gap-1.5 capitalize">
                                <Icon aria-hidden="true" className="size-3.5" />
                                {v}
                              </span>
                              <span className="font-normal text-muted-foreground">{text}</span>
                            </Label>
                          </div>
                        );
                      })}
                    </RadioGroup>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="default_branch"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Default branch</FormLabel>
                  <FormControl>
                    <Input autoComplete="off" className="font-mono" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormError error={create.error} />
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="outline">
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" disabled={create.isPending}>
                {create.isPending ? "Creating…" : "Create tree"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
