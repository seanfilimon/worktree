import { useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { cn } from "@/lib/utils";

interface SplitPaneProps {
  start: ReactNode;
  end: ReactNode;
  /** Accessible name of the separator. */
  label: string;
  /** Initial width of the start pane in pixels. */
  defaultSize?: number;
  min?: number;
  max?: number;
  className?: string;
}

const STEP = 16;

/**
 * Two horizontally resizable panes (WAI-ARIA window splitter: drag the
 * handle, or focus it and use the arrow / Home / End keys). Below the `md`
 * breakpoint the panes stack and the handle is hidden.
 */
export function SplitPane({ start, end, label, defaultSize = 288, min = 200, max = 560, className }: SplitPaneProps) {
  const [size, setSize] = useState(defaultSize);
  const drag = useRef<{ x: number; size: number } | null>(null);
  const clamp = (v: number) => Math.min(max, Math.max(min, Math.round(v)));

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    drag.current = { x: e.clientX, size };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (drag.current) setSize(clamp(drag.current.size + e.clientX - drag.current.x));
  };
  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    drag.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const next =
      e.key === "ArrowLeft" ? size - STEP : e.key === "ArrowRight" ? size + STEP : e.key === "Home" ? min : e.key === "End" ? max : null;
    if (next === null) return;
    e.preventDefault();
    setSize(clamp(next));
  };

  return (
    <div
      style={{ "--split-start": `${size}px` } as CSSProperties}
      className={cn("grid min-w-0 grid-cols-1 gap-3 md:grid-cols-[var(--split-start)_auto_minmax(0,1fr)] md:gap-0", className)}
    >
      <div className="min-w-0">{start}</div>
      <div
        role="separator"
        aria-label={label}
        aria-orientation="vertical"
        aria-valuenow={size}
        aria-valuemin={min}
        aria-valuemax={max}
        tabIndex={0}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onKeyDown={onKeyDown}
        className="group hidden w-3 cursor-col-resize touch-none items-stretch justify-center outline-none md:flex"
      >
        <span className="w-px bg-border transition-colors group-hover:w-0.5 group-hover:bg-foreground group-focus-visible:w-0.5 group-focus-visible:bg-foreground" />
      </div>
      <div className="min-w-0">{end}</div>
    </div>
  );
}
