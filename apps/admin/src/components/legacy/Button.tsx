import type { ButtonHTMLAttributes, Ref } from "react";
import { Link, type LinkProps } from "react-router";
import { cn } from "@/lib/cn";
import { Spinner } from "./Spinner";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-primary text-primary-foreground border-transparent hover:brightness-110",
  secondary: "bg-surface text-fg border-border-strong hover:bg-surface-2",
  ghost: "bg-transparent text-muted-foreground border-transparent hover:bg-surface-2 hover:text-fg",
  danger: "bg-danger text-white border-transparent hover:brightness-110",
};

const SIZES: Record<Size, string> = {
  sm: "h-7 px-2.5 text-[13px] gap-1.5",
  md: "h-8 px-3 text-sm gap-2",
};

function buttonClass(variant: Variant = "secondary", size: Size = "md", className?: string): string {
  return cn(
    "inline-flex shrink-0 items-center justify-center rounded-md border font-medium whitespace-nowrap transition-colors",
    "disabled:cursor-not-allowed disabled:opacity-50",
    VARIANTS[variant],
    SIZES[size],
    className,
  );
}

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  ref?: Ref<HTMLButtonElement>;
  variant?: Variant;
  size?: Size;
  loading?: boolean;
}

export function Button({ variant, size, loading = false, className, children, disabled, type = "button", ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      className={buttonClass(variant, size, className)}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading && <Spinner className="size-3.5" />}
      {children}
    </button>
  );
}

export function ButtonLink({
  variant,
  size,
  className,
  ...rest
}: LinkProps & { variant?: Variant; size?: Size }) {
  return <Link className={buttonClass(variant, size, className)} {...rest} />;
}
