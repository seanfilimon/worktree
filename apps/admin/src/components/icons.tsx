import type { SVGProps } from "react";
import { cn } from "@/lib/cn";

/** 16px stroke icons (decorative; always aria-hidden). */
const PATHS = {
  dashboard: "M2.5 2.5h4.5v6h-4.5zM9 2.5h4.5v3.5H9zM9 8h4.5v5.5H9zM2.5 10.5h4.5v3H2.5z",
  tenants: "M2.5 13.5v-9l5-2 5 2v9M5.5 6.5h1M9.5 6.5h1M5.5 9.5h1M9.5 9.5h1M1.5 13.5h13",
  trees: "M8 14V8M8 8L4.5 4.5M8 8l3.5-3.5M4.5 4.5a1.5 1.5 0 1 0 0-.01M11.5 4.5a1.5 1.5 0 1 0 0-.01M8 14a1.2 1.2 0 1 0 0-.01",
  audit: "M4 2.5h6l2.5 2.5v8.5H4zM6 7h4.5M6 9.5h4.5M6 12h2.5",
  accounts: "M8 8a2.75 2.75 0 1 0 0-5.5A2.75 2.75 0 0 0 8 8zM2.5 14a5.5 5.5 0 0 1 11 0",
  settings: "M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4zM8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4",
  menu: "M2.5 4h11M2.5 8h11M2.5 12h11",
  close: "M4 4l8 8M12 4l-8 8",
  folder: "M1.5 4.5v8h13v-6.5H8L6.5 4.5z",
  file: "M4 1.5h5l3 3v10H4zM9 1.5v3h3",
  chevron: "M6 4l4 4-4 4",
  logout: "M6 2.5H3v11h3M10.5 5l3 3-3 3M13.5 8H6",
  lock: "M4.5 7V5a3.5 3.5 0 0 1 7 0v2M3.5 7h9v6.5h-9z",
  branch: "M4.5 2v12M4.5 10c0-3 7-2 7-6M11.5 4a1.5 1.5 0 1 0 0-.01",
  download: "M8 2v8M4.5 7L8 10.5 11.5 7M2.5 13.5h11",
  copy: "M5.5 5.5h8v8h-8zM2.5 10.5v-8h8",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, className, ...rest }: { name: IconName } & SVGProps<SVGSVGElement>) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn("size-4 shrink-0", className)}
      {...rest}
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
