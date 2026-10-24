import { useEffect, useRef, useState } from "react";
import { Button, type ButtonProps } from "./Button";

interface ConfirmButtonProps extends Omit<ButtonProps, "onClick"> {
  /** Label of the confirming button, e.g. "Confirm revoke". */
  confirmLabel: string;
  onConfirm: () => void;
}

/**
 * Two-step destructive action: the first click reveals Confirm / Cancel
 * inline, keeping focus on the confirm control. Escape cancels.
 */
export function ConfirmButton({ confirmLabel, onConfirm, children, variant = "ghost", size = "sm", ...rest }: ConfirmButtonProps) {
  const [armed, setArmed] = useState(false);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (armed) confirmRef.current?.focus();
  }, [armed]);

  if (!armed) {
    return (
      <Button ref={triggerRef} variant={variant} size={size} onClick={() => setArmed(true)} {...rest}>
        {children}
      </Button>
    );
  }

  const cancel = () => {
    setArmed(false);
    queueMicrotask(() => triggerRef.current?.focus());
  };

  return (
    <span
      className="inline-flex items-center gap-1"
      onKeyDown={(e) => {
        if (e.key === "Escape") cancel();
      }}
    >
      <Button
        ref={confirmRef}
        variant="danger"
        size={size}
        loading={rest.loading}
        onClick={() => {
          setArmed(false);
          onConfirm();
        }}
      >
        {confirmLabel}
      </Button>
      <Button variant="ghost" size={size} onClick={cancel}>
        Cancel
      </Button>
    </span>
  );
}
