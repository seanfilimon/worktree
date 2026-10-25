import { ImageOff } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { EmptyState } from "./states";

/**
 * Previews raster image bytes through a short-lived blob URL (the blob
 * endpoint needs the bearer token, so a plain <img src> cannot be used).
 */
export function ImagePreview({ bytes, type, alt }: { bytes: ArrayBuffer; type: string; alt: string }) {
  const ref = useRef<HTMLImageElement>(null);
  const [state, setState] = useState<{ w: number; h: number } | "error" | null>(null);

  useEffect(() => {
    const img = ref.current;
    if (!img) return;
    const url = URL.createObjectURL(new Blob([bytes], { type }));
    img.src = url;
    return () => {
      img.removeAttribute("src");
      URL.revokeObjectURL(url);
    };
  }, [bytes, type]);

  return (
    <figure className="flex flex-col items-center gap-2 p-6">
      {state === "error" && (
        <EmptyState icon={ImageOff} title="Cannot display image">
          The file is not a valid {type.replace("image/", "").toUpperCase()} image.
        </EmptyState>
      )}
      <div
        hidden={state === "error"}
        className="max-w-full overflow-auto rounded-md border bg-[repeating-conic-gradient(var(--muted)_0_25%,var(--background)_0_50%)] bg-[length:16px_16px] p-2"
      >
        <img
          ref={ref}
          alt={alt}
          onLoad={(e) => setState({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })}
          onError={() => setState("error")}
          className="block h-auto max-h-[70vh] max-w-full object-contain"
        />
      </div>
      {state && state !== "error" && (
        <figcaption className="text-xs text-muted-foreground tabular-nums">
          {state.w} × {state.h} px
        </figcaption>
      )}
    </figure>
  );
}
