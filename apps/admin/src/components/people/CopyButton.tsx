import { Check, Copy } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { useToast } from "@/toast/store";

interface CopyButtonProps {
  value: string;
  /** Accessible name, e.g. "Copy token". */
  label: string;
  /** Toast title on success. */
  copiedMessage?: string;
}

export function CopyButton({ value, label, copiedMessage = "Copied to clipboard" }: CopyButtonProps) {
  const toast = useToast();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const id = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(id);
  }, [copied]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      toast.success(copiedMessage);
    } catch {
      toast.push({ kind: "error", title: "Copy failed", message: "Select the text and copy it manually." });
    }
  };

  return (
    <Button type="button" variant="outline" size="sm" onClick={() => void copy()} aria-label={label}>
      {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
      {copied ? "Copied" : "Copy"}
    </Button>
  );
}
