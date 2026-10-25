import { useId, type ComponentProps } from "react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

interface AuthFieldProps extends Omit<ComponentProps<typeof Input>, "id"> {
  label: string;
  hint?: string;
  error?: string;
}

/** Labelled input with hint / error text wired up for assistive tech. */
export function AuthField({ label, hint, error, ...input }: AuthFieldProps) {
  const id = useId();
  const note = error ?? hint;
  return (
    <div className="grid gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input
        id={id}
        aria-invalid={error ? true : undefined}
        aria-describedby={note ? `${id}-note` : undefined}
        {...input}
      />
      {note && (
        <p
          id={`${id}-note`}
          className={error ? "text-xs font-medium text-foreground" : "text-xs text-muted-foreground"}
        >
          {error}
          {!error && hint}
        </p>
      )}
    </div>
  );
}
