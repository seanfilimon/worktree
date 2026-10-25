import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation } from "@tanstack/react-query";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { useApi } from "@/api/context";
import { PasswordField } from "@/components/people/PasswordField";
import { FormError } from "@/components/people/states";
import { MIN_PASSWORD_LENGTH } from "@/components/people/validation";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { useToast } from "@/toast/store";

const schema = z
  .object({
    current: z.string().min(1, "Enter your current password."),
    next: z.string().min(MIN_PASSWORD_LENGTH, `At least ${MIN_PASSWORD_LENGTH} characters.`),
    confirm: z.string(),
  })
  .refine((v) => v.next !== v.current, { path: ["next"], message: "Choose a password different from the current one." })
  .refine((v) => v.confirm === v.next, { path: ["confirm"], message: "Passwords do not match." });

type Values = z.infer<typeof schema>;

const EMPTY: Values = { current: "", next: "", confirm: "" };

export function ChangePasswordCard() {
  const api = useApi();
  const toast = useToast();
  const form = useForm<Values>({ resolver: zodResolver(schema), defaultValues: EMPTY });

  const change = useMutation({
    mutationFn: (v: Values) => api.auth.changePassword({ current_password: v.current, new_password: v.next }),
    meta: { inlineError: true },
    onSuccess: () => {
      toast.success("Password changed", "Your other sessions were signed out.");
      form.reset(EMPTY);
    },
  });

  return (
    <Card>
      <Form {...form}>
        <form
          aria-label="Change password"
          onSubmit={(e) => void form.handleSubmit((v) => change.mutate(v))(e)}
          className="flex flex-col gap-6"
          noValidate
        >
          <CardHeader>
            <CardTitle>Change password</CardTitle>
            <CardDescription>At least {MIN_PASSWORD_LENGTH} characters. Changing it signs out your other sessions.</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4 sm:max-w-md">
            <FormField
              control={form.control}
              name="current"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Current password</FormLabel>
                  <FormControl>
                    <PasswordField autoComplete="current-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="next"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>New password</FormLabel>
                  <FormControl>
                    <PasswordField autoComplete="new-password" meter {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="confirm"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Confirm new password</FormLabel>
                  <FormControl>
                    <PasswordField autoComplete="new-password" {...field} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormError error={change.error} />
          </CardContent>
          <CardFooter className="justify-end border-t pt-6">
            <Button type="submit" disabled={change.isPending}>
              {change.isPending ? "Saving…" : "Change password"}
            </Button>
          </CardFooter>
        </form>
      </Form>
    </Card>
  );
}
