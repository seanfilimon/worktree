import type { MouseEvent, ReactNode } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: ReactNode;
  confirmLabel: string;
  /** Resolves when done; the dialog closes on success and stays open on failure. */
  onConfirm: () => Promise<unknown>;
  pending?: boolean;
}

/** Controlled confirmation for irreversible or access-reducing actions. */
export function ConfirmDialog({ open, onOpenChange, title, description, confirmLabel, onConfirm, pending = false }: ConfirmDialogProps) {
  const confirm = async (e: MouseEvent) => {
    // Keep the dialog open until the action settles.
    e.preventDefault();
    try {
      await onConfirm();
      onOpenChange(false);
    } catch {
      // The mutation's error is reported by the global error toast.
    }
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription>{description}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>Cancel</AlertDialogCancel>
          <AlertDialogAction disabled={pending} onClick={(e) => void confirm(e)}>
            {confirmLabel}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
