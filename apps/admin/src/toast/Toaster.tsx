import { useEffect, useRef, useSyncExternalStore } from "react";
import { toast } from "sonner";
import { Toaster as SonnerToaster } from "@/components/ui/sonner";
import { useToast } from "./store";

/**
 * Renders the toast store with Sonner (monochrome; the kind is conveyed by
 * the icon). Store entries map to Sonner toasts one-to-one: new entries are
 * shown, removed entries are dismissed, and Sonner closing a toast removes
 * its entry.
 */
export function Toaster() {
  const store = useToast();
  const items = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
  /** Store toast id -> Sonner toast id. */
  const shown = useRef(new Map<number, string | number>());

  useEffect(() => {
    const map = shown.current;
    const live = new Set(items.map((t) => t.id));
    for (const [id, sonnerId] of map) {
      if (!live.has(id)) {
        map.delete(id);
        toast.dismiss(sonnerId);
      }
    }
    for (const t of items) {
      if (map.has(t.id)) continue;
      const remove = () => store.dismiss(t.id);
      map.set(
        t.id,
        toast[t.kind](t.title, {
          description: t.message,
          duration: store.autoDismissMs > 0 ? store.autoDismissMs : Number.POSITIVE_INFINITY,
          onDismiss: remove,
          onAutoClose: remove,
        }),
      );
    }
  }, [items, store]);

  useEffect(() => {
    const map = shown.current;
    return () => {
      for (const sonnerId of map.values()) toast.dismiss(sonnerId);
      map.clear();
    };
  }, []);

  return <SonnerToaster position="bottom-right" closeButton />;
}
