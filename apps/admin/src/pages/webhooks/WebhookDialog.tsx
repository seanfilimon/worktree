import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, TriangleAlert } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { EVENT_SUGGESTIONS, webhookFormErrors } from "./webhookForm";
import { useV06Api, v06Keys, type CreatedWebhook, type UpdateWebhookRequest, type Webhook, type WebhookScope } from "@/api/v06";
import { ChipsInput } from "@/components/files/ChipsInput";
import { CopyButton } from "@/components/people/CopyButton";
import { FormError } from "@/components/people/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { useToast } from "@/toast/store";

interface WebhookDialogProps {
  scope: WebhookScope;
  /** null = create. */
  webhook: Webhook | null;
  onClose: () => void;
}

/** Create or edit a webhook. A server-generated secret is shown once after creation. */
export function WebhookDialog({ scope, webhook, onClose }: WebhookDialogProps) {
  const api = useV06Api();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const editing = webhook !== null;
  const [url, setUrl] = useState(webhook?.url ?? "");
  const [description, setDescription] = useState(webhook?.description ?? "");
  const [events, setEvents] = useState<string[]>(webhook?.events ?? ["branch.pushed"]);
  const [secret, setSecret] = useState("");
  const [active, setActive] = useState(webhook?.active ?? true);
  const [touched, setTouched] = useState(false);
  const [created, setCreated] = useState<CreatedWebhook | null>(null);
  const errors = webhookFormErrors(url, events);

  const save = useMutation({
    mutationFn: async () => {
      if (webhook) {
        const body: UpdateWebhookRequest = { url: url.trim(), description: description.trim(), events, active };
        if (secret.trim()) body.secret = secret.trim();
        return api.webhooks.update(scope, webhook.id, body);
      }
      return api.webhooks.create(scope, {
        url: url.trim(),
        events,
        active,
        ...(description.trim() && { description: description.trim() }),
        ...(secret.trim() && { secret: secret.trim() }),
      });
    },
    meta: { inlineError: true },
    onSuccess: async (w: CreatedWebhook) => {
      toast.success(editing ? "Webhook updated" : "Webhook created", w.url);
      await queryClient.invalidateQueries({ queryKey: v06Keys.webhooks(scope) });
      if (!editing && w.secret) setCreated(w);
      else onClose();
    },
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.url || errors.events) return;
    save.mutate();
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        {created?.secret ? (
          <>
            <DialogHeader>
              <DialogTitle>Webhook created</DialogTitle>
              <DialogDescription>
                Deliveries to <span className="font-mono break-all text-foreground">{created.url}</span> are signed with this secret (
                <span className="font-mono">X-WT-Signature: sha256=…</span>).
              </DialogDescription>
            </DialogHeader>
            <Alert>
              <TriangleAlert aria-hidden="true" />
              <AlertTitle>Copy the secret now</AlertTitle>
              <AlertDescription>This is the only time it is shown. Store it where the receiving service verifies signatures.</AlertDescription>
            </Alert>
            <div className="flex items-center gap-2">
              <Label htmlFor={`${id}-created-secret`} className="sr-only">
                Signing secret
              </Label>
              <Input
                id={`${id}-created-secret`}
                readOnly
                value={created.secret}
                className="font-mono text-xs"
                onFocus={(e) => e.target.select()}
              />
              <CopyButton value={created.secret} label="Copy secret" copiedMessage="Secret copied" />
            </div>
            <DialogFooter>
              <Button onClick={onClose}>Done</Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>{editing ? "Edit webhook" : "Add webhook"}</DialogTitle>
              <DialogDescription>
                W0rkTree POSTs a JSON event to the URL for every matching event in{" "}
                <span className="font-mono text-foreground">{scope.tree ? `${scope.tenant}/${scope.tree}` : scope.tenant}</span>.
              </DialogDescription>
            </DialogHeader>
            <form id={`${id}-form`} onSubmit={submit} aria-label={editing ? "Edit webhook" : "Add webhook"} className="grid gap-4" noValidate>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-url`}>Payload URL</Label>
                <Input
                  id={`${id}-url`}
                  type="url"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="https://ci.example.com/hooks/worktree"
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono"
                  aria-invalid={touched && errors.url ? true : undefined}
                  aria-describedby={`${id}-url-hint`}
                />
                <p id={`${id}-url-hint`} className={touched && errors.url ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
                  {(touched && errors.url) || "Private and loopback addresses are refused unless the server allows them."}
                </p>
              </div>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-description`}>Description</Label>
                <Textarea
                  id={`${id}-description`}
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  rows={2}
                  maxLength={500}
                  placeholder="Trigger CI builds"
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-events`}>Events</Label>
                <ChipsInput
                  id={`${id}-events`}
                  values={events}
                  onChange={setEvents}
                  placeholder="branch.pushed, mr.*"
                  describedBy={`${id}-events-hint`}
                  normalize={(s) => s.trim().toLowerCase()}
                />
                <p id={`${id}-events-hint`} className={touched && errors.events ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
                  {(touched && errors.events) || "Event types or patterns: mr.* matches every merge request event, * every event."}
                </p>
                <div className="flex flex-wrap gap-1" aria-label="Suggested events">
                  {EVENT_SUGGESTIONS.filter((e) => !events.includes(e)).map((e) => (
                    <Button
                      key={e}
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-6 px-2 font-mono text-[11px]"
                      aria-label={`Add event ${e}`}
                      onClick={() => setEvents((prev) => [...prev, e])}
                    >
                      <Plus aria-hidden="true" className="size-3" />
                      {e}
                    </Button>
                  ))}
                </div>
              </div>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-secret`}>{editing ? "New secret" : "Secret"}</Label>
                <Input
                  id={`${id}-secret`}
                  type="password"
                  value={secret}
                  onChange={(e) => setSecret(e.target.value)}
                  autoComplete="new-password"
                  spellCheck={false}
                  className="font-mono"
                  aria-describedby={`${id}-secret-hint`}
                />
                <p id={`${id}-secret-hint`} className="text-xs text-muted-foreground">
                  {editing
                    ? webhook?.has_secret
                      ? "Leave empty to keep the current secret."
                      : "This webhook has no secret: deliveries are unsigned."
                    : "Leave empty to have the server generate one. It is shown once."}
                </p>
              </div>
              <div className="flex items-start justify-between gap-4 rounded-md border p-3">
                <div className="grid gap-1">
                  <Label htmlFor={`${id}-active`}>Active</Label>
                  <p id={`${id}-active-desc`} className="text-xs text-muted-foreground">
                    Inactive webhooks receive nothing. Repeated failures deactivate a webhook automatically.
                  </p>
                </div>
                <Switch id={`${id}-active`} checked={active} onCheckedChange={setActive} aria-describedby={`${id}-active-desc`} />
              </div>
              <FormError error={save.error} />
            </form>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Cancel
              </Button>
              <Button type="submit" form={`${id}-form`} disabled={save.isPending}>
                {save.isPending ? "Saving…" : editing ? "Save webhook" : "Add webhook"}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
