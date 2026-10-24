import { useId, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

const controlClass = cn(
  "w-full rounded-md border border-border-strong bg-surface px-2.5 text-sm text-fg placeholder:text-subtle",
  "focus:border-foreground focus:outline-none focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-ring",
  "disabled:cursor-not-allowed disabled:opacity-60 aria-[invalid=true]:border-danger",
);

interface FieldProps {
  label: string;
  hint?: ReactNode;
  error?: string;
  className?: string;
  /** Renders the label visually hidden (still read by screen readers). */
  hideLabel?: boolean;
  children: (ids: { id: string; describedBy: string | undefined; invalid: boolean }) => ReactNode;
}

/** Label + control + hint/error wiring with correct aria attributes. */
export function Field({ label, hint, error, className, hideLabel = false, children }: FieldProps) {
  const id = useId();
  const hintId = hint ? `${id}-hint` : undefined;
  const errorId = error ? `${id}-error` : undefined;
  const describedBy = [hintId, errorId].filter(Boolean).join(" ") || undefined;
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <label htmlFor={id} className={cn("text-[13px] font-medium text-fg", hideLabel && "sr-only")}>
        {label}
      </label>
      {children({ id, describedBy, invalid: Boolean(error) })}
      {hint && (
        <p id={hintId} className="text-xs text-muted-foreground">
          {hint}
        </p>
      )}
      {error && (
        <p id={errorId} className="text-xs text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

type WithField<T> = T & { label: string; hint?: ReactNode; error?: string; fieldClassName?: string; hideLabel?: boolean };

export function TextField({ label, hint, error, fieldClassName, hideLabel, className, ...rest }: WithField<InputHTMLAttributes<HTMLInputElement>>) {
  return (
    <Field label={label} hint={hint} error={error} className={fieldClassName} hideLabel={hideLabel}>
      {({ id, describedBy, invalid }) => (
        <input
          id={id}
          aria-describedby={describedBy}
          aria-invalid={invalid || undefined}
          className={cn(controlClass, "h-8", className)}
          {...rest}
        />
      )}
    </Field>
  );
}

export function TextAreaField({ label, hint, error, fieldClassName, hideLabel, className, ...rest }: WithField<TextareaHTMLAttributes<HTMLTextAreaElement>>) {
  return (
    <Field label={label} hint={hint} error={error} className={fieldClassName} hideLabel={hideLabel}>
      {({ id, describedBy, invalid }) => (
        <textarea
          id={id}
          aria-describedby={describedBy}
          aria-invalid={invalid || undefined}
          className={cn(controlClass, "min-h-20 py-1.5", className)}
          {...rest}
        />
      )}
    </Field>
  );
}

export function SelectField({ label, hint, error, fieldClassName, hideLabel, className, children, ...rest }: WithField<SelectHTMLAttributes<HTMLSelectElement>>) {
  return (
    <Field label={label} hint={hint} error={error} className={fieldClassName} hideLabel={hideLabel}>
      {({ id, describedBy, invalid }) => (
        <select
          id={id}
          aria-describedby={describedBy}
          aria-invalid={invalid || undefined}
          className={cn(controlClass, "h-8 pr-7", className)}
          {...rest}
        >
          {children}
        </select>
      )}
    </Field>
  );
}

export function Checkbox({ label, hint, className, ...rest }: InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: string }) {
  const id = useId();
  return (
    <div className={cn("flex items-start gap-2", className)}>
      <input id={id} type="checkbox" className="mt-0.5 size-4 rounded accent-(--color-accent)" aria-describedby={hint ? `${id}-hint` : undefined} {...rest} />
      <div>
        <label htmlFor={id} className="text-sm font-medium">
          {label}
        </label>
        {hint && (
          <p id={`${id}-hint`} className="text-xs text-muted-foreground">
            {hint}
          </p>
        )}
      </div>
    </div>
  );
}
