import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleX, Loader2, MonitorSmartphone, ShieldAlert } from "lucide-react";
import { useId, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { useSearchParams } from "react-router";
import { isApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { securityKeys } from "@/api/security";
import {
  createDeviceApi,
  deviceKeys,
  formatUserCodeInput,
  normalizeUserCode,
  type DeviceRequest,
} from "@/api/device";
import { useAuth } from "@/auth/context";
import { PageHeader } from "@/components/PageHeader";
import { FormError, QueryError } from "@/components/people/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { absoluteTime, relativeTime } from "@/lib/format";
import { describeUserAgent } from "@/pages/security/userAgent";

function useDeviceApi() {
  const api = useApi();
  return useMemo(() => createDeviceApi(api.client), [api]);
}

/** Step 1: enter (or confirm the prefilled) user code. */
function CodeForm({ initial, onSubmit }: { initial: string; onSubmit: (code: string) => void }) {
  const id = useId();
  const [value, setValue] = useState(() => formatUserCodeInput(initial));
  const [error, setError] = useState<string | null>(null);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const code = normalizeUserCode(value);
    if (!code) {
      setError("Enter the 8-letter code shown on your device, for example BCDF-GHJK.");
      return;
    }
    setError(null);
    onSubmit(code);
  };

  return (
    <Card className="max-w-lg">
      <form onSubmit={submit} noValidate>
        <CardHeader>
          <CardTitle>Connect a device</CardTitle>
          <CardDescription>
            Enter the code printed by <code className="font-mono text-xs">wt login --device</code> on the device you
            want to sign in.
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-2 pt-6">
          <Label htmlFor={id}>Device code</Label>
          <Input
            id={id}
            value={value}
            onChange={(e) => setValue(formatUserCodeInput(e.target.value))}
            placeholder="XXXX-XXXX"
            autoComplete="off"
            autoCapitalize="characters"
            spellCheck={false}
            maxLength={9}
            className="font-mono text-lg tracking-[0.3em] uppercase"
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? `${id}-error` : undefined}
          />
          {error && (
            <p id={`${id}-error`} className="text-xs font-medium">
              {error}
            </p>
          )}
        </CardContent>
        <CardFooter className="justify-end pt-6">
          <Button type="submit">Continue</Button>
        </CardFooter>
      </form>
    </Card>
  );
}

function Detail({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="grid gap-1 sm:grid-cols-[10rem_1fr] sm:gap-4">
      <dt className="text-sm text-muted-foreground">{term}</dt>
      <dd className="min-w-0 text-sm break-words">{children}</dd>
    </div>
  );
}

/** The outcome after approving or denying (or opening a decided code). */
function Outcome({ status, onAnother }: { status: "approved" | "denied"; onAnother: () => void }) {
  const approved = status === "approved";
  const Icon = approved ? CircleCheck : CircleX;
  return (
    <Card className="max-w-lg" role="status">
      <CardHeader>
        <div className="flex items-center gap-3">
          <Icon aria-hidden="true" className="size-6" />
          <CardTitle>{approved ? "Device approved" : "Sign-in denied"}</CardTitle>
        </div>
        <CardDescription>
          {approved
            ? "You can return to your terminal: wt finishes signing in on its own within a few seconds."
            : "The device was not signed in. Start again on the device if this was a mistake."}
        </CardDescription>
      </CardHeader>
      <CardFooter className="justify-end pt-6">
        <Button variant="outline" onClick={onAnother}>
          Enter another code
        </Button>
      </CardFooter>
    </Card>
  );
}

/** Step 2: review the request and approve or deny it. */
function ReviewCard({ request, onDone, onBack }: { request: DeviceRequest; onDone: (r: DeviceRequest) => void; onBack: () => void }) {
  const device = useDeviceApi();
  const { account } = useAuth();
  const mfaId = useId();
  const [mfaCode, setMfaCode] = useState("");
  const [mfaError, setMfaError] = useState<string | null>(null);
  const scopes = request.scopes ?? [];

  const approve = useMutation({
    mutationFn: () => device.approve(request.user_code, mfaCode.replace(/\s/g, "") || undefined),
    onSuccess: onDone,
  });
  const deny = useMutation({ mutationFn: () => device.deny(request.user_code), onSuccess: onDone });
  const busy = approve.isPending || deny.isPending;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (request.mfa_required && !mfaCode.trim()) {
      setMfaError("Enter the 6-digit code from your authenticator app or a recovery code.");
      return;
    }
    setMfaError(null);
    approve.mutate();
  };

  const error = approve.error ?? deny.error;
  const expired = isApiError(error) && error.status === 404;

  return (
    <Card className="max-w-2xl">
      <form onSubmit={submit} noValidate>
        <CardHeader>
          <div className="flex items-center gap-3">
            <MonitorSmartphone aria-hidden="true" className="size-5" />
            <CardTitle>Approve this device?</CardTitle>
          </div>
          <CardDescription>
            <span className="font-medium text-foreground">{request.client_name}</span> wants to sign in to your account
            {account ? (
              <>
                {" "}
                <span className="font-medium text-foreground">{account.username}</span>
              </>
            ) : null}
            .
          </CardDescription>
        </CardHeader>
        <CardContent className="grid gap-6 pt-6">
          <dl className="grid gap-3" aria-label="Device request">
            <Detail term="Code">
              <span className="font-mono tracking-widest">{request.user_code}</span>
            </Detail>
            {request.client_id && (
              <Detail term="Client">
                <span className="font-mono text-xs">{request.client_id}</span>
              </Detail>
            )}
            <Detail term="Access">
              {scopes.length === 0 ? (
                <span>Full access to your account</span>
              ) : (
                <ul className="flex flex-wrap gap-1.5" aria-label="Requested scopes">
                  {scopes.map((s) => (
                    <li key={s}>
                      <Badge variant="outline" className="font-mono">
                        {s}
                      </Badge>
                    </li>
                  ))}
                </ul>
              )}
            </Detail>
            {(request.remote_ip || request.user_agent) && (
              <Detail term="Requested from">
                {[request.remote_ip, request.user_agent ? describeUserAgent(request.user_agent) : ""]
                  .filter(Boolean)
                  .join(" · ")}
              </Detail>
            )}
            <Detail term="Expires">
              <time dateTime={request.expires_at} title={absoluteTime(request.expires_at)}>
                {relativeTime(request.expires_at)}
              </time>
            </Detail>
          </dl>

          <Alert>
            <ShieldAlert aria-hidden="true" />
            <AlertTitle>Only approve a sign-in you started</AlertTitle>
            <AlertDescription>
              Check that the code matches the one on your device. Approving gives that device the access listed above
              until you revoke its session.
            </AlertDescription>
          </Alert>

          {request.mfa_required && (
            <div className="grid gap-2">
              <Label htmlFor={mfaId}>Two-factor code</Label>
              <Input
                id={mfaId}
                value={mfaCode}
                onChange={(e) => setMfaCode(e.target.value)}
                inputMode="text"
                autoComplete="one-time-code"
                placeholder="123456"
                className="max-w-48 font-mono"
                aria-invalid={mfaError ? true : undefined}
                aria-describedby={`${mfaId}-note`}
              />
              <p id={`${mfaId}-note`} className={mfaError ? "text-xs font-medium" : "text-xs text-muted-foreground"}>
                {mfaError ?? "Your session was not created with a second factor; confirm with a TOTP or recovery code."}
              </p>
            </div>
          )}

          {error &&
            (expired ? (
              <Alert>
                <CircleX aria-hidden="true" />
                <AlertTitle>This code has expired</AlertTitle>
                <AlertDescription>Start the sign-in again on your device to get a new code.</AlertDescription>
              </Alert>
            ) : (
              <FormError error={error} />
            ))}
        </CardContent>
        <CardFooter className="flex-wrap justify-between gap-2 pt-6">
          <Button type="button" variant="ghost" onClick={onBack} disabled={busy}>
            Use another code
          </Button>
          <div className="flex gap-2">
            <Button type="button" variant="outline" disabled={busy} onClick={() => deny.mutate()}>
              {deny.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
              Deny
            </Button>
            <Button type="submit" disabled={busy}>
              {approve.isPending && <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
              Approve
            </Button>
          </div>
        </CardFooter>
      </form>
    </Card>
  );
}

/** Looks up a code, then shows the review card or its outcome. */
function DeviceReview({ code, onBack }: { code: string; onBack: () => void }) {
  const device = useDeviceApi();
  const queryClient = useQueryClient();
  const [decided, setDecided] = useState<DeviceRequest | null>(null);
  const request = useQuery({
    queryKey: deviceKeys.request(code),
    queryFn: () => device.get(code),
    retry: false,
    staleTime: Infinity,
  });

  if (decided && decided.status !== "pending") {
    return <Outcome status={decided.status} onAnother={onBack} />;
  }
  if (request.isPending) {
    return (
      <Card className="max-w-2xl" role="status" aria-label="Looking up the code">
        <CardHeader>
          <Skeleton className="h-6 w-56" />
          <Skeleton className="h-4 w-72" />
        </CardHeader>
        <CardContent className="grid gap-3 pt-6">
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-2/3" />
        </CardContent>
      </Card>
    );
  }
  if (request.isError) {
    if (isApiError(request.error) && request.error.status === 404) {
      return (
        <Card className="max-w-lg" role="alert">
          <CardHeader>
            <CardTitle>Code not found</CardTitle>
            <CardDescription>
              <span className="font-mono">{code}</span> is unknown or has expired. Codes are valid for 10 minutes; start
              the sign-in again on your device for a new one.
            </CardDescription>
          </CardHeader>
          <CardFooter className="justify-end pt-6">
            <Button variant="outline" onClick={onBack}>
              Enter another code
            </Button>
          </CardFooter>
        </Card>
      );
    }
    return (
      <Card className="max-w-lg">
        <QueryError error={request.error} onRetry={() => void request.refetch()} title="Could not look up the code" />
      </Card>
    );
  }
  if (request.data.status !== "pending") {
    return <Outcome status={request.data.status} onAnother={onBack} />;
  }
  return (
    <ReviewCard
      request={request.data}
      onBack={onBack}
      onDone={(r) => {
        setDecided(r);
        void queryClient.invalidateQueries({ queryKey: securityKeys.sessions });
      }}
    />
  );
}

/** Approve a device sign-in (wt login --device) by its user code (§16.6). */
export function DevicePage() {
  const [params, setParams] = useSearchParams();
  // verification_uri_complete (?code=XXXX-XXXX) prefills the form; the
  // user still confirms it (RFC 8628 §5.4).
  const [prefill, setPrefill] = useState(() => params.get("code") ?? "");
  const [code, setCode] = useState<string | null>(null);

  const back = () => {
    setCode(null);
    setPrefill("");
    if (params.has("code")) setParams({}, { replace: true });
  };

  return (
    <div className="mx-auto w-full max-w-3xl">
      <PageHeader
        title="Device sign-in"
        description="Sign in the W0rkTree CLI on another machine without typing your password there."
      />
      {code ? <DeviceReview key={code} code={code} onBack={back} /> : <CodeForm initial={prefill} onSubmit={setCode} />}
    </div>
  );
}
