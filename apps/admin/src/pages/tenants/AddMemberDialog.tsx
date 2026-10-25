import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import { ROLES } from "@/api/types";
import { ROLE_SUMMARY } from "@/components/people/roles";
import { FormError } from "@/components/people/states";
import { USERNAME_HINT, USERNAME_RE } from "@/components/people/validation";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useToast } from "@/toast/store";

const schema = z.object({
  username: z
    .string()
    .trim()
    .toLowerCase()
    .regex(USERNAME_RE, USERNAME_HINT),
  role: z.enum(ROLES),
});

type Values = z.infer<typeof schema>;

const EMPTY: Values = { username: "", role: "developer" };

interface AddMemberDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  slug: string;
  canGrantOwner: boolean;
}

export function AddMemberDialog({ open, onOpenChange, slug, canGrantOwner }: AddMemberDialogProps) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: EMPTY });

  const add = useMutation({
    mutationFn: (v: Values) => api.tenants.setMember(slug, v.username, v.role),
    meta: { inlineError: true },
    onSuccess: async (m) => {
      toast.success("Member added", `${m.account.username} joined ${slug} as ${m.role}`);
      await queryClient.invalidateQueries({ queryKey: keys.members(slug) });
      changeOpen(false);
    },
  });

  function changeOpen(next: boolean) {
    if (!next) {
      form.reset(EMPTY);
      add.reset();
    }
    onOpenChange(next);
  }

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add member</DialogTitle>
          <DialogDescription>
            Give an existing account access to <span className="font-mono text-foreground">{slug}</span>.
          </DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={(e) => void form.handleSubmit((v) => add.mutate(v))(e)} className="flex flex-col gap-4" noValidate>
            <FormField
              control={form.control}
              name="username"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Username</FormLabel>
                  <FormControl>
                    <Input placeholder="jane" autoComplete="off" spellCheck={false} className="font-mono" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="role"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Role</FormLabel>
                  <Select value={field.value} onValueChange={field.onChange}>
                    <FormControl>
                      <SelectTrigger className="w-full capitalize">
                        <SelectValue />
                      </SelectTrigger>
                    </FormControl>
                    <SelectContent>
                      {ROLES.map((r) => (
                        <SelectItem key={r} value={r} disabled={r === "owner" && !canGrantOwner} className="capitalize">
                          {r}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <FormDescription>{ROLE_SUMMARY[field.value]}</FormDescription>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormError error={add.error} />
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="outline">
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" disabled={add.isPending}>
                {add.isPending ? "Adding…" : "Add member"}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
