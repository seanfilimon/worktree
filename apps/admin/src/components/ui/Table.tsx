import type { ReactNode, TdHTMLAttributes, ThHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

export function Table({ children, label, className }: { children: ReactNode; label: string; className?: string }) {
  return (
    <div className={cn("overflow-x-auto", className)}>
      <table aria-label={label} className="w-full border-collapse text-left text-[13px]">
        {children}
      </table>
    </div>
  );
}

export function Th({ className, ...rest }: ThHTMLAttributes<HTMLTableCellElement>) {
  return (
    <th
      scope="col"
      className={cn("border-b border-border bg-surface-2/60 px-4 py-2 text-xs font-medium tracking-wide text-muted uppercase", className)}
      {...rest}
    />
  );
}

export function Td({ className, ...rest }: TdHTMLAttributes<HTMLTableCellElement>) {
  return <td className={cn("border-b border-border px-4 py-2 align-middle", className)} {...rest} />;
}

export function Tr({ children, className }: { children: ReactNode; className?: string }) {
  return <tr className={cn("last:[&>td]:border-b-0 hover:bg-surface-2/50", className)}>{children}</tr>;
}
