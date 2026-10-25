import { create } from "qrcode";
import { useMemo } from "react";
import { cn } from "@/lib/utils";

interface QrCodeProps {
  value: string;
  /** Accessible name of the image. */
  label: string;
  className?: string;
}

const QUIET_ZONE = 4;

/**
 * Client-side QR code (the secret never leaves the browser), drawn as one
 * SVG path of black modules on a white field: scanners need dark-on-light
 * regardless of the panel theme, and the design stays monochrome.
 */
export function QrCode({ value, label, className }: QrCodeProps) {
  const { size, path } = useMemo(() => {
    const { modules } = create(value, { errorCorrectionLevel: "M" });
    let d = "";
    for (let row = 0; row < modules.size; row++) {
      for (let col = 0; col < modules.size; col++) {
        if (modules.get(row, col)) d += `M${col + QUIET_ZONE} ${row + QUIET_ZONE}h1v1h-1z`;
      }
    }
    return { size: modules.size + QUIET_ZONE * 2, path: d };
  }, [value]);

  return (
    <svg
      role="img"
      aria-label={label}
      viewBox={`0 0 ${size} ${size}`}
      shapeRendering="crispEdges"
      className={cn("size-44 rounded-md border bg-white", className)}
    >
      <rect width={size} height={size} fill="#ffffff" />
      <path d={path} fill="#000000" />
    </svg>
  );
}
